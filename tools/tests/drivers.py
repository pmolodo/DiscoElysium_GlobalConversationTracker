"""Loads a driver from tools/ so a test can call into it.

THE DRIVERS ARE NAMED WITH HYPHENS - `measure-menus.py`, `menu-costs-diff.py` - because they
are commands before they are modules, and a command with an underscore in it reads wrongly on a
command line. A hyphen is not an identifier, so `import measure_menus` cannot reach them and
every test that wants one has to go through importlib. Renaming the drivers to suit the tests
would be the tail wagging the dog; one loader here costs less.

A driver also expects `tools/` on the path, since it imports `measurement_common` as a sibling.
That is done once, here, rather than in each test module.
"""

import importlib.util
import sys

from pathlib import Path

TOOLS = Path(__file__).resolve().parent.parent

if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))


def load(name):
    """The driver `tools/<name>.py`, as a module.

    Loaded fresh on each call rather than cached in `sys.modules`, so one test cannot leave a
    driver's module-level state where the next one finds it.
    """
    path = TOOLS / f"{name}.py"
    if not path.exists():
        raise FileNotFoundError(f"no driver at {path}")
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), path)
    if spec is None or spec.loader is None:
        raise ImportError(f"{path} exists but Python will not load it as a module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
