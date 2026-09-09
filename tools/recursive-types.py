#!/usr/bin/env python

"""Find every recursive or self-referential type in the crate, from rustdoc's own JSON.

## Why rustdoc JSON rather than a parse of the sources

Whether a type is recursive is decidable from the types, and deriving the list any other way
finds only what somebody already noticed. A regex over `src/` cannot resolve `use`, aliases
or re-exports, and grepping for `Box<Self>` finds the self-loops while missing exactly the
case a hand search always misses - mutual recursion between two or three types. rustdoc has
already resolved every path to an item id, so the graph built here is the compiler's view.

Regenerate the input with:

    RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items" \\
      cargo +nightly rustdoc --lib

`--document-private-items` is not optional: most of this crate's types are private, and
without it the analysis silently reports on a fraction of them.

## What an edge is

An edge runs from a type to every type mentioned in its fields or variants, looked THROUGH
the generic containers rather than at them - `Box<T>`, `Vec<T>`, `Option<T>`, `HashMap<K, V>`,
`[T; N]`, `&T`, `Rc<T>`, tuples and so on all yield edges to their arguments. Any cycle is a
recursive type.

## The two cases this takes a position on

TRAIT OBJECTS. `Box<dyn Trait>` gets an edge to the TRAIT, and every type implementing that
trait gets an edge from it - so a cycle closed only through a trait object is found. That
over-approximates: an implementor that never actually appears behind that particular box
still closes a cycle here. Over-approximating is the right direction for an audit, and such
cycles are reported separately so they can be read as the weaker finding they are.

GENERIC PARAMETERS. The graph is GENERIC, not monomorphic: a field of type `T` yields no
edge, since `T` is a parameter rather than a type. A cycle that exists only for a particular
instantiation is therefore not reported. This crate instantiates its own generics with its
own concrete types, which the graph already carries, so the gap is narrow - but it is a gap.

## The self-check

In Rust a recursive type can only be sized if its cycle passes through an indirection, so
every cycle must run through a `Box`, `Vec`, `Rc`, `Arc`, `HashMap` or similar. A reported
cycle with no indirection on it means this analysis is wrong, because the compiler would have
rejected the type - so that is checked and reported rather than assumed.
"""

import argparse
import json
import sys
import traceback

from pathlib import Path

###############################################################################
# Core functions
###############################################################################

# Containers whose arguments a cycle may run through. Rust requires one of these on every
# cycle, so this doubles as the self-check below.
INDIRECTIONS = {
    "Box",
    "Vec",
    "VecDeque",
    "Rc",
    "Arc",
    "RefCell",
    "Cell",
    "Mutex",
    "RwLock",
    "HashMap",
    "BTreeMap",
    "HashSet",
    "BTreeSet",
    "Option",
    "Result",
}

# Traits that say nothing about what a value HOLDS. Every type implements most of these, so
# expanding a trait object into its implementors through one of them connects the whole graph
# to itself and reports cycles that are an artefact of the analysis.
MARKERS = {
    "Send",
    "Sync",
    "Unpin",
    "Sized",
    "Copy",
    "Clone",
    "Debug",
    "Display",
    "Default",
    "Eq",
    "PartialEq",
    "Ord",
    "PartialOrd",
    "Hash",
    "Drop",
    "Serialize",
    "Deserialize",
    "RefUnwindSafe",
    "UnwindSafe",
    "Freeze",
    "Any",
    "ToOwned",
    "From",
    "Into",
}


def load(path):
    """The rustdoc index, keyed by item id."""
    with open(path, "r", encoding="utf-8") as handle:
        doc = json.load(handle)
    return doc


# rustdoc's `index` is keyed by the STRING form of an id while every id inside an item is an
# INTEGER. Mixing the two silently returns nothing from every lookup, which is a graph with
# no edges and a report saying nothing is recursive - so ids are normalised at the boundary
# and nowhere else.
def key(item_id):
    return str(item_id)


def mentioned(kind):
    """Every item id a type expression refers to, looked through the containers.

    rustdoc's type JSON is a tagged union; this walks it whole rather than matching the
    shapes it expects, so a form that is not handled yields nothing instead of raising.
    """
    found = set()
    if not isinstance(kind, (dict, list)):
        return found
    if isinstance(kind, list):
        for item in kind:
            found |= mentioned(item)
        return found

    for name, value in kind.items():
        if name == "resolved_path" and isinstance(value, dict):
            if "id" in value:
                found.add(key(value["id"]))
            found |= mentioned(value.get("args"))
        elif name == "dyn_trait" and isinstance(value, dict):
            # THE PRINCIPAL TRAIT ONLY. A `dyn` type lists its auto-trait bounds - Send,
            # Sync, Unpin - beside the trait that gives it its behaviour, and an edge to one
            # of those says nothing about what the object holds. It is also actively wrong
            # once the impl pass below runs, since everything implements Send: an edge into
            # Send and Send's edges back out to every implementor close a cycle through any
            # type at all. Measured here first as a three-type "cycle" of Send, Sync and a
            # struct holding one boxed closure.
            for trait in value.get("traits", []):
                path = trait.get("trait", {})
                if "id" not in path:
                    continue
                if (path.get("path") or "").split("::")[-1] in MARKERS:
                    continue
                found.add(key(path["id"]))
                break
        else:
            found |= mentioned(value)
    return found


def type_graph(doc):
    """A node per type definition, and the edges out of each.

    Returns (edges, names, kinds) where edges maps an id to the ids it refers to.
    """
    index = doc["index"]
    edges = {}
    names = {}
    kinds = {}

    for item_id, item in index.items():
        inner = item.get("inner")
        if not isinstance(inner, dict):
            continue
        for kind in ("struct", "enum", "union", "type_alias"):
            if kind in inner:
                break
        else:
            continue

        names[item_id] = item.get("name") or "<unnamed>"
        kinds[item_id] = kind
        edges.setdefault(item_id, set())

    # A second pass, so a field naming a type defined later still yields an edge.
    for item_id in list(edges):
        body = index[item_id]["inner"][kinds[item_id]]

        # Fields and variants are reached through their own item ids rather than read off
        # the body, which carries only the ids.
        for child in child_ids(body, index):
            child_item = index.get(child)
            if child_item is None:
                continue
            edges[item_id] |= mentioned(child_item.get("inner"))
        # A type alias has no children; its whole body is a type.
        if kinds[item_id] == "type_alias":
            edges[item_id] |= mentioned(body.get("type"))

    # Trait objects: an edge from the trait to everything implementing it, so a cycle
    # closed through `Box<dyn Trait>` is visible. Over-approximates on purpose.
    for item in index.values():
        inner = item.get("inner")
        if not isinstance(inner, dict) or "impl" not in inner:
            continue
        impl = inner["impl"]
        trait = impl.get("trait")
        if not trait or "id" not in trait:
            continue
        # A blanket or synthetic impl is rustdoc telling us about a bound rather than about
        # a type this crate wrote, and a marker trait is implemented by everything.
        if impl.get("blanket_impl") or impl.get("is_synthetic"):
            continue
        if (trait.get("path") or "").split("::")[-1] in MARKERS:
            continue
        trait_id = key(trait["id"])
        for implementor in mentioned(impl.get("for")):
            if implementor in edges:
                edges.setdefault(trait_id, set()).add(implementor)
                names.setdefault(trait_id, trait.get("path", "<trait>").split("::")[-1])
                kinds.setdefault(trait_id, "trait")

    return edges, names, kinds


def child_ids(body, index):
    """Every `struct_field` item id under a struct or enum body.

    A STRUCT'S FIELDS ARE UNDER `kind`, NOT AT THE TOP - `{"struct": {"kind": {"plain":
    {"fields": [...]}}}}` for a named struct and `{"kind": {"tuple": [...]}}` for a tuple
    one. Reading `body["fields"]` finds nothing and finds it silently, which reports every
    struct as having no fields at all and therefore no type as recursive. An enum's
    `variants` ARE at the top, which is what makes the omission easy to miss: enums come out
    right while structs come out empty.

    TWO LEVELS FOR AN ENUM. Its `variants` are variant items, and a variant's own `kind`
    holds the field items - a list for a tuple variant and under `fields` for a struct one.
    A unit variant holds neither, and a field rustdoc stripped is a null rather than an id.
    """
    found = []
    for child in body.get("variants") or []:
        if child is not None:
            found.append(key(child))
    found.extend(fields_of(body.get("kind")))

    for child in list(found):
        item = index.get(child)
        if item is None:
            continue
        inner = item.get("inner")
        if not isinstance(inner, dict) or "variant" not in inner:
            continue
        found.extend(fields_of(inner["variant"].get("kind")))
    return found


def fields_of(kind):
    """The field item ids under a struct-or-variant `kind`, whichever shape it takes.

    `plain` and `struct` hold them under `fields`; `tuple` is the list itself; `unit` holds
    none. A stripped field is a null and is skipped rather than counted.
    """
    found = []
    if not isinstance(kind, dict):
        return found
    for shape in ("plain", "struct"):
        body = kind.get(shape)
        if isinstance(body, dict):
            for field in body.get("fields") or []:
                if field is not None:
                    found.append(key(field))
    for field in kind.get("tuple") or []:
        if field is not None:
            found.append(key(field))
    return found


def cycles(edges):
    """Every strongly connected component with a cycle in it, by Tarjan's algorithm.

    Iterative rather than recursive, which is the joke this script is entitled to: an audit
    for recursive structures should not overflow its own stack on a deep one.
    """
    index_of = {}
    low = {}
    on_stack = {}
    stack = []
    result = []
    counter = [0]

    for root in list(edges):
        if root in index_of:
            continue
        work = [(root, iter(sorted(edges.get(root, ()))))]
        index_of[root] = low[root] = counter[0]
        counter[0] += 1
        stack.append(root)
        on_stack[root] = True

        while work:
            node, children = work[-1]
            advanced = False
            for child in children:
                if child not in edges:
                    continue
                if child not in index_of:
                    index_of[child] = low[child] = counter[0]
                    counter[0] += 1
                    stack.append(child)
                    on_stack[child] = True
                    work.append((child, iter(sorted(edges.get(child, ())))))
                    advanced = True
                    break
                if on_stack.get(child):
                    low[node] = min(low[node], index_of[child])
            if advanced:
                continue

            work.pop()
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[node])
            if low[node] == index_of[node]:
                component = []
                while True:
                    member = stack.pop()
                    on_stack[member] = False
                    component.append(member)
                    if member == node:
                        break
                if len(component) > 1 or node in edges.get(node, ()):
                    result.append(sorted(component))

    return result


def indirections_on(component, doc, edges, names):
    """Which indirection containers appear on a component's own edges.

    The self-check: Rust cannot size a cycle that has none, so an empty answer means this
    analysis built an edge the compiler would have rejected.
    """
    index = doc["index"]
    found = set()
    members = set(component)
    for item_id in component:
        item = index.get(item_id)
        if item is None:
            continue
        text = json.dumps(item.get("inner"))
        for container in INDIRECTIONS:
            if f'"{container}"' in text:
                found.add(container)
        # A field's own item carries the type, so look there too.
        inner = item.get("inner")
        if isinstance(inner, dict):
            for kind in ("struct", "enum", "union"):
                if kind in inner:
                    for child in child_ids(inner[kind], index):
                        child_item = index.get(child)
                        if child_item is None:
                            continue
                        if not (mentioned(child_item.get("inner")) & members):
                            continue
                        child_text = json.dumps(child_item.get("inner"))
                        for container in INDIRECTIONS:
                            if f'"{container}"' in container_names(child_text):
                                found.add(container)
    return found


def container_names(text):
    """The text, so a container name can be looked for in it. Kept separate to name why."""
    return text


def where_defined(doc, item_id):
    """The source file and line a type is defined at, as the report has to point somewhere."""
    item = doc["index"].get(item_id, {})
    span = item.get("span")
    if not span:
        return "?"
    return f"{span.get('filename', '?')}:{span.get('begin', ['?'])[0]}"


def report(doc, edges, names, kinds, out):
    found = cycles(edges)
    ours = []
    for component in found:
        # A component holding no type of ours is third-party and reported apart: the remedy
        # there is containment rather than a rewrite.
        if any(is_ours(doc, member) for member in component):
            ours.append(component)

    print(f"type definitions in the graph   {len(edges)}", file=out)
    print(f"recursive components            {len(found)}", file=out)
    print(f"... holding a type of ours      {len(ours)}", file=out)
    print(file=out)

    if not ours:
        print(
            "NOTHING OF OURS IS RECURSIVE, which is a result rather than an empty run -"
            " see the\nheader for what would have been found if it were.",
            file=out,
        )

    for component in sorted(ours, key=lambda c: -len(c)):
        shape = "self-loop" if len(component) == 1 else f"mutual, {len(component)} types"
        print(f"=== {shape}", file=out)
        for member in component:
            mark = " " if is_ours(doc, member) else "*"
            print(
                f"  {mark} {names.get(member, '?'):<28} {kinds.get(member, '?'):<11} {where_defined(doc, member)}",
                file=out,
            )
        carriers = indirections_on(component, doc, edges, names)
        if carriers:
            print(f"    through: {', '.join(sorted(carriers))}", file=out)
        else:
            print("    THROUGH NOTHING - this analysis is wrong here, because Rust cannot size such a type", file=out)
        print(file=out)

    print("* marks a type that is not ours; the remedy for those is containment.", file=out)
    foreign(doc, edges, names, out)


# Crates whose types recurse in ways this crate cannot change, listed so a holder of one is
# reported rather than passed over. oxidd is the reason src/symbolic/isolated.rs exists:
# releasing a large diagram walks it recursively, so a thread that holds one wants room.
WATCHED = ("oxidd",)


def foreign(doc, edges, names, out):
    """Our types that hold a type from a watched third-party crate.

    ## Why this is a separate list rather than more nodes in the graph

    `cargo rustdoc --lib` documents THIS crate, so a dependency's types appear in `paths`
    with a name and a crate but never in `index` with fields. Their internal edges are
    therefore invisible here, and a cycle running through one cannot be found however the
    graph is built - the honest report is which of our types hold one, not a claim about
    what those types do inside.

    The remedy for these is containment rather than a rewrite, which is what `isolated.rs`
    already does, so they are reported apart from the cycles.
    """
    paths = doc.get("paths", {})
    holders = {}
    for item_id in edges:
        if not is_ours(doc, item_id):
            continue
        for referred in edges[item_id]:
            path = paths.get(referred)
            if path is None:
                continue
            crate = (path.get("path") or ["?"])[0]
            if crate in WATCHED:
                holders.setdefault(names.get(item_id, "?"), set()).add("::".join(path.get("path") or []))

    print(file=out)
    print("HOLDERS OF A THIRD-PARTY TYPE THAT RECURSES, which the graph above cannot reach into:", file=out)
    if not holders:
        print("  none", file=out)
        return
    for holder in sorted(holders):
        print(f"  {holder:<28} {', '.join(sorted(holders[holder]))}", file=out)


def is_ours(doc, item_id):
    """Whether a type is defined in this crate rather than pulled in from a dependency."""
    item = doc["index"].get(item_id)
    if item is None:
        return False
    span = item.get("span")
    if not span:
        return False
    name = span.get("filename", "")
    return not ("\\.cargo\\" in name or "/.cargo/" in name or "\\rustlib\\" in name)


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "json",
        nargs="?",
        default="target/doc/lookahead_engine.json",
        help="rustdoc JSON for the crate",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        path = Path(args.json)
        if not path.exists():
            print(f"no rustdoc JSON at {path}; see this script's header for how to make it", file=sys.stderr)
            return 2
        doc = load(path)
        edges, names, kinds = type_graph(doc)
        report(doc, edges, names, kinds, sys.stdout)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
