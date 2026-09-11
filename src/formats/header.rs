// SPDX-License-Identifier: MIT
//! What a file this repository wrote says about itself, and the one rule for reading it.
//!
//! TWO FIELDS, ONE FACT. A document names the format it is written in and which version of
//! that format it is, and a reader checks both or neither. The failure a check prevents is
//! not a corrupt file - it is a file of the WRONG KIND being read as the right one: these
//! shapes are all objects of objects, so a diff read as a state, or a manifest read as a
//! diff, parses, yields something, and loses whatever did not happen to line up. Inferring
//! what a document is from what it contains is exactly what a name at the top of it exists
//! to stop.
//!
//! ONLY THE CURRENT VERSION IS READ. Not "anything not newer" - the current one, and
//! nothing else. A reader that still understands an older shape is a place where that shape
//! ROTS, because nothing else exercises it; a converter is a place where it is written down
//! and tested. Every file this repository commits is at the current version, so nothing
//! here is turned away by that rule, and the rule is in place before there is a second
//! version for it to be wrong about.

use std::fmt;

/// What a document calls the format it is written in.
pub const FORMAT_KEY: &str = "_format";

/// What a document calls the version of that format.
pub const VERSION_KEY: &str = "_formatVersion";

/// What a diff calls the document it is a diff of.
///
/// A diff that cannot say what it changes is readable only through whatever manifest
/// happens to sit beside it. The path is relative to the diff itself, and what it names may
/// be a diff in turn.
pub const BASE_KEY: &str = "_base";

/// How to bring a file that is refused up to date, named in every message.
///
/// A STRICT READER WITHOUT A SIGNPOSTED CONVERTER IS A WALL. "your file is version 2" is
/// not an instruction, and the person reading the refusal is the one who has to act on it.
pub const CONVERTER: &str = "dotnet run --project tools/FormatConvert -- <file>";

/// Why a document could not be read as the format a reader wanted.
///
/// Its `Display` is written out by hand rather than derived, because two of these say
/// different things depending on their own contents: an older version is convertible and a
/// newer one is not, and the remedy is the interesting half of the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderFault {
    /// It says nothing about what it is.
    Unnamed { wanted: String },
    /// It says it is something else.
    Mismatched { wanted: String, found: String },
    /// It says what it is and not which version.
    Unstamped { format: String },
    /// It is a version this build does not write.
    Version {
        format: String,
        found: u32,
        current: u32,
    },
}

impl fmt::Display for HeaderFault {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unnamed { wanted } => write!(
                out,
                "it does not say what format it is written in, and a {FORMAT_KEY} of \
                 '{wanted}' is what this reads",
            ),
            Self::Mismatched { wanted, found } => {
                write!(out, "it is a '{found}' document, and this reads '{wanted}'")
            }
            Self::Unstamped { format } => write!(
                out,
                "it is a {format} and carries no {VERSION_KEY}; convert it first:\n  \
                 {CONVERTER}",
            ),
            // BOTH DIRECTIONS, and they are different failures with different remedies. A
            // file from the FUTURE is one this build cannot fully understand and must not
            // touch. A file from the PAST is one the converter can bring forward.
            Self::Version {
                format,
                found,
                current,
            } if found > current => write!(
                out,
                "this {format} is version {found} and this build writes version {current}. \
                 It was written by a newer build; the file is not damaged, so do not \
                 overwrite it - use a build at least as new as the one that wrote it",
            ),
            Self::Version {
                format,
                found,
                current,
            } => write!(
                out,
                "this {format} is version {found} and this build reads only version \
                 {current}. It is not damaged and nothing in it is lost - convert it \
                 first:\n  {CONVERTER}",
            ),
        }
    }
}

impl std::error::Error for HeaderFault {}

/// The header a reader expects to find, and the check itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    /// The format the reader is written for.
    pub format: &'static str,
    /// The version of it this build writes.
    pub version: u32,
}

impl Expected {
    /// Checks what a document says it is against what the reader wants.
    ///
    /// # Errors
    ///
    /// Where the document names no format, names another one, carries no version, or
    /// carries any version but the current one.
    pub fn check(&self, found: Option<&str>, version: Option<u32>) -> Result<(), HeaderFault> {
        let Some(found) = found else {
            return Err(HeaderFault::Unnamed {
                wanted: self.format.to_string(),
            });
        };

        if found != self.format {
            return Err(HeaderFault::Mismatched {
                wanted: self.format.to_string(),
                found: found.to_string(),
            });
        }

        let Some(version) = version else {
            return Err(HeaderFault::Unstamped {
                format: self.format.to_string(),
            });
        };

        if version != self.version {
            return Err(HeaderFault::Version {
                format: self.format.to_string(),
                found: version,
                current: self.version,
            });
        }

        Ok(())
    }

    /// The same, against a JSON document that may or may not carry a header at all.
    ///
    /// # Errors
    ///
    /// As [`Expected::check`], and where either field is of the wrong JSON type - a version
    /// written as a string is a file to stop for rather than one to guess about.
    pub fn check_document(&self, document: &serde_json::Value) -> Result<(), HeaderFault> {
        let named = document.get(FORMAT_KEY);
        let stamped = document.get(VERSION_KEY);

        let found = match named {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(value.as_str().ok_or_else(|| HeaderFault::Mismatched {
                wanted: self.format.to_string(),
                found: value.to_string(),
            })?),
        };

        let version = match stamped {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(
                u32::try_from(value.as_u64().ok_or_else(|| HeaderFault::Version {
                    format: self.format.to_string(),
                    found: 0,
                    current: self.version,
                })?)
                .map_err(|_| HeaderFault::Version {
                    format: self.format.to_string(),
                    found: u32::MAX,
                    current: self.version,
                })?,
            ),
        };

        self.check(found, version)
    }

    /// The two fields, ready to be written at the top of a document of this format.
    pub fn stamp(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut header = serde_json::Map::new();
        header.insert(FORMAT_KEY.to_string(), self.format.into());
        header.insert(VERSION_KEY.to_string(), self.version.into());
        header
    }
}

/// Whether a document carries a header at all, which is what tells a diff from a whole file.
pub fn is_stamped(document: &serde_json::Value) -> bool {
    document
        .get(FORMAT_KEY)
        .is_some_and(|named| !named.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANTED: Expected = Expected {
        format: "json-diff",
        version: 1,
    };

    #[test]
    fn the_current_version_of_the_right_format_reads() {
        assert_eq!(WANTED.check(Some("json-diff"), Some(1)), Ok(()));
    }

    #[test]
    fn a_document_of_another_format_is_refused_by_name() {
        let fault = WANTED.check(Some("sparse-diff"), Some(1)).unwrap_err();

        assert!(fault.to_string().contains("sparse-diff"));
        assert!(fault.to_string().contains("json-diff"));
    }

    #[test]
    fn a_document_that_says_nothing_is_refused_rather_than_assumed() {
        assert_eq!(
            WANTED.check(None, Some(1)),
            Err(HeaderFault::Unnamed {
                wanted: "json-diff".to_string()
            })
        );
    }

    #[test]
    fn an_unstamped_document_is_refused_and_pointed_at_the_converter() {
        let fault = WANTED.check(Some("json-diff"), None).unwrap_err();

        assert!(fault.to_string().contains(CONVERTER));
    }

    /// Both directions are refused, and the two say different things to do about it.
    #[test]
    fn any_version_but_the_current_one_is_refused() {
        let older = WANTED.check(Some("json-diff"), Some(0)).unwrap_err();
        let newer = WANTED.check(Some("json-diff"), Some(2)).unwrap_err();

        assert!(older.to_string().contains(CONVERTER));
        assert!(newer.to_string().contains("do not overwrite it"));
        assert!(!newer.to_string().contains(CONVERTER));
    }

    #[test]
    fn a_whole_document_is_checked_the_same_way() {
        let document = serde_json::json!({ "_format": "json-diff", "_formatVersion": 1 });

        assert_eq!(WANTED.check_document(&document), Ok(()));
        assert!(is_stamped(&document));
    }

    #[test]
    fn a_document_with_no_header_is_not_stamped() {
        assert!(!is_stamped(&serde_json::json!({ "version": 4 })));
    }

    #[test]
    fn a_version_that_is_not_a_number_is_refused_rather_than_guessed_at() {
        let document = serde_json::json!({ "_format": "json-diff", "_formatVersion": "1" });

        assert!(WANTED.check_document(&document).is_err());
    }

    #[test]
    fn the_stamp_is_what_a_reader_would_accept() {
        let mut document = serde_json::Map::new();
        document.extend(WANTED.stamp());

        assert_eq!(
            WANTED.check_document(&serde_json::Value::Object(document)),
            Ok(())
        );
    }
}
