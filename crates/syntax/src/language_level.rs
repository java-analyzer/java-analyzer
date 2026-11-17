//! Java language levels (`--release`, `--enable-preview`) and the feature availability table.
//! See `docs/specs/0003-java-parser-conformance-testing.md`.

use std::{fmt, str::FromStr};

use rowan::{NodeOrToken, WalkEvent};

use crate::{
    syntax_kind::SyntaxKind::{self, *},
    SyntaxError, SyntaxNode,
};

/// A Java feature release version, clamped to [`Release::FLOOR`] (`8`) at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release(u16);

impl Release {
    /// Oldest officially supported LTS release (Java 8 as of 2026-11).
    pub const FLOOR: Self = Self(8);
    /// Newest known feature release.
    pub const LATEST: Self = Self(27);

    pub const JAVA_8: Self = Self(8);
    pub const JAVA_9: Self = Self(9);
    pub const JAVA_10: Self = Self(10);
    pub const JAVA_11: Self = Self(11);
    pub const JAVA_12: Self = Self(12);
    pub const JAVA_13: Self = Self(13);
    pub const JAVA_14: Self = Self(14);
    pub const JAVA_15: Self = Self(15);
    pub const JAVA_16: Self = Self(16);
    pub const JAVA_17: Self = Self(17);
    pub const JAVA_18: Self = Self(18);
    pub const JAVA_19: Self = Self(19);
    pub const JAVA_20: Self = Self(20);
    pub const JAVA_21: Self = Self(21);
    pub const JAVA_22: Self = Self(22);
    pub const JAVA_23: Self = Self(23);
    pub const JAVA_24: Self = Self(24);
    pub const JAVA_25: Self = Self(25);
    pub const JAVA_26: Self = Self(26);
    pub const JAVA_27: Self = Self(27);

    /// Creates a [`Release`], clamping values below [`Release::FLOOR`] to the floor.
    pub const fn new(number: u16) -> Self {
        Self::from_declared(number).0
    }

    /// Converts a declared release number to a [`Release`], clamping below [`Release::FLOOR`] and
    /// returning whether clamping occurred.
    pub const fn from_declared(number: u16) -> (Self, bool) {
        if number < Self::FLOOR.0 {
            (Self::FLOOR, true)
        } else {
            (Self(number), false)
        }
    }

    pub const fn number(self) -> u16 {
        self.0
    }
}

impl Default for Release {
    fn default() -> Self {
        Self::LATEST
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Release {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.strip_prefix("1.").unwrap_or(s);
        let n = digits
            .parse::<u16>()
            .map_err(|_| format!("invalid Java release `{s}`"))?;
        if n == 0 {
            return Err(format!("invalid Java release `{s}`"));
        }
        Ok(Self::new(n))
    }
}

/// Declared language level of a compilation unit (`--release N` and optional `--enable-preview`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LanguageLevel {
    pub release: Release,
    pub preview: bool,
}

impl LanguageLevel {
    pub const fn new(release: Release, preview: bool) -> Self {
        Self { release, preview }
    }

    pub const fn latest() -> Self {
        Self {
            release: Release::LATEST,
            preview: false,
        }
    }

    pub fn is_available(self, feature: &Feature) -> bool {
        feature.is_available(self)
    }

    pub fn diagnostic(self, feature: &Feature) -> Option<String> {
        feature.diagnostic(self)
    }
}

impl Default for LanguageLevel {
    fn default() -> Self {
        Self::latest()
    }
}

/// One language feature introduced after [`Release::FLOOR`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub id: &'static str,
    /// Plural noun phrase used in diagnostics, e.g. `"text blocks"` or `"records"`.
    pub plural: &'static str,
    pub jep: Option<u16>,
    pub preview_in: &'static [Release],
    pub stable_in: Option<Release>,
    pub withdrawn_in: Option<Release>,
}

impl Feature {
    pub fn by_id(id: &str) -> Option<&'static Feature> {
        FEATURES.iter().find(|f| f.id == id)
    }

    pub fn is_available(&self, level: LanguageLevel) -> bool {
        if let Some(withdrawn) = self.withdrawn_in {
            if level.release >= withdrawn {
                return false;
            }
        }
        if let Some(stable) = self.stable_in {
            if level.release >= stable {
                return true;
            }
        }
        level.preview && self.preview_in.contains(&level.release)
    }

    /// Returns the syntax diagnostic message when this feature is used at `level`, or `None` if
    /// the feature is available at `level`.
    pub fn diagnostic(&self, level: LanguageLevel) -> Option<String> {
        if self.is_available(level) {
            return None;
        }
        if let Some(withdrawn) = self.withdrawn_in {
            if level.release >= withdrawn {
                return Some(format!(
                    "{} were removed in language level {}",
                    self.plural,
                    withdrawn.number()
                ));
            }
        }
        if self.preview_in.contains(&level.release) {
            return Some(format!(
                "{} are a preview feature in java {}; enable preview",
                self.plural,
                level.release.number()
            ));
        }
        if let Some(stable) = self.stable_in {
            return Some(format!(
                "{} require language level {}",
                self.plural,
                stable.number()
            ));
        }
        if let Some(first_preview) = self.preview_in.first() {
            return Some(format!(
                "{} require language level {} with preview enabled",
                self.plural,
                first_preview.number()
            ));
        }
        Some(format!(
            "{} are not available at language level {}",
            self.plural,
            level.release.number()
        ))
    }
}

pub const SWITCH_EXPRESSIONS: Feature = Feature {
    id: "switch-expressions",
    plural: "switch expressions",
    jep: Some(361),
    preview_in: &[Release::JAVA_12, Release::JAVA_13],
    stable_in: Some(Release::JAVA_14),
    withdrawn_in: None,
};

pub const TEXT_BLOCKS: Feature = Feature {
    id: "text-blocks",
    plural: "text blocks",
    jep: Some(378),
    preview_in: &[Release::JAVA_13, Release::JAVA_14],
    stable_in: Some(Release::JAVA_15),
    withdrawn_in: None,
};

pub const RECORDS: Feature = Feature {
    id: "records",
    plural: "records",
    jep: Some(395),
    preview_in: &[Release::JAVA_14, Release::JAVA_15],
    stable_in: Some(Release::JAVA_16),
    withdrawn_in: None,
};

pub const PATTERN_MATCHING_INSTANCEOF: Feature = Feature {
    id: "pattern-matching-instanceof",
    plural: "instanceof patterns",
    jep: Some(394),
    preview_in: &[Release::JAVA_14, Release::JAVA_15],
    stable_in: Some(Release::JAVA_16),
    withdrawn_in: None,
};

pub const SEALED_CLASSES: Feature = Feature {
    id: "sealed-classes",
    plural: "sealed classes",
    jep: Some(409),
    preview_in: &[Release::JAVA_15, Release::JAVA_16],
    stable_in: Some(Release::JAVA_17),
    withdrawn_in: None,
};

pub const RECORD_PATTERNS: Feature = Feature {
    id: "record-patterns",
    plural: "record patterns",
    jep: Some(440),
    preview_in: &[Release::JAVA_19, Release::JAVA_20],
    stable_in: Some(Release::JAVA_21),
    withdrawn_in: None,
};

pub const PATTERN_MATCHING_SWITCH: Feature = Feature {
    id: "pattern-matching-switch",
    plural: "switch patterns",
    jep: Some(441),
    preview_in: &[
        Release::JAVA_17,
        Release::JAVA_18,
        Release::JAVA_19,
        Release::JAVA_20,
    ],
    stable_in: Some(Release::JAVA_21),
    withdrawn_in: None,
};

pub const UNNAMED_VARIABLES: Feature = Feature {
    id: "unnamed-variables",
    plural: "unnamed variables and patterns",
    jep: Some(456),
    preview_in: &[Release::JAVA_21],
    stable_in: Some(Release::JAVA_22),
    withdrawn_in: None,
};

/// All language features tracked after [`Release::FLOOR`].
pub const FEATURES: &[Feature] = &[
    SWITCH_EXPRESSIONS,
    TEXT_BLOCKS,
    RECORDS,
    PATTERN_MATCHING_INSTANCEOF,
    SEALED_CLASSES,
    RECORD_PATTERNS,
    PATTERN_MATCHING_SWITCH,
    UNNAMED_VARIABLES,
];

/// Checks `root` for constructs whose feature is unavailable at `level`, appending syntax-phase
/// diagnostics to `errors`.
pub(crate) fn check_feature_gates(
    root: &SyntaxNode,
    level: LanguageLevel,
    errors: &mut Vec<SyntaxError>,
) {
    // Fast path: when at or above the newest stable release in the table, every standard feature
    // is available.
    if FEATURES.iter().all(|f| f.is_available(level)) {
        return;
    }

    let mut report = |feature: &Feature, range| {
        if let Some(message) = feature.diagnostic(level) {
            errors.push(SyntaxError { message, range });
        }
    };

    for event in root.preorder_with_tokens() {
        let WalkEvent::Enter(element) = event else {
            continue;
        };
        match element {
            NodeOrToken::Token(token) => match token.kind() {
                TEXT_BLOCK => report(&TEXT_BLOCKS, token.text_range()),
                NON_SEALED_KW => report(&SEALED_CLASSES, token.text_range()),
                IDENT
                    if token.text() == "sealed"
                        && token
                            .parent()
                            .is_some_and(|p| p.kind() == MODIFIER_LIST) =>
                {
                    report(&SEALED_CLASSES, token.text_range());
                }
                UNDERSCORE_KW
                    if token.parent().is_some_and(|p| {
                        matches!(
                            p.kind(),
                            VARIABLE_DECLARATOR
                                | FOREACH_STMT
                                | RESOURCE
                                | CATCH_CLAUSE
                                | PARAM
                                | TYPE_PATTERN
                                | UNNAMED_PATTERN
                        )
                    }) =>
                {
                    report(&UNNAMED_VARIABLES, token.text_range());
                }
                _ => {}
            },
            NodeOrToken::Node(node) => match node.kind() {
                RECORD_DECL => report(&RECORDS, node.text_range()),
                PERMITS_CLAUSE => report(&SEALED_CLASSES, node.text_range()),
                SWITCH_EXPR | YIELD_STMT => report(&SWITCH_EXPRESSIONS, node.text_range()),
                SWITCH_STMT => {
                    if let Some(label) = first_enhanced_switch_label(&node) {
                        report(&SWITCH_EXPRESSIONS, label.text_range());
                    }
                }
                SWITCH_LABEL => {
                    if is_pattern_switch_label(&node) {
                        report(&PATTERN_MATCHING_SWITCH, node.text_range());
                    }
                }
                INSTANCEOF_EXPR => {
                    if node
                        .children()
                        .any(|c| matches!(c.kind(), TYPE_PATTERN | RECORD_PATTERN))
                    {
                        report(&PATTERN_MATCHING_INSTANCEOF, node.text_range());
                    }
                }
                RECORD_PATTERN => report(&RECORD_PATTERNS, node.text_range()),
                _ => {}
            },
        }
    }
}

/// First `SWITCH_LABEL` of a `SWITCH_STMT` that uses Java 14 switch-expression syntax (`->` or
/// comma-separated case items).
fn first_enhanced_switch_label(switch_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    let body = switch_stmt
        .children()
        .find(|c| c.kind() == SWITCH_BODY)?;
    for case in body.children().filter(|c| c.kind() == SWITCH_CASE) {
        if let Some(label) = case.children().find(|c| c.kind() == SWITCH_LABEL) {
            let enhanced = label.children_with_tokens().any(|e| {
                matches!(
                    e.kind(),
                    SyntaxKind::ARROW | SyntaxKind::COMMA
                )
            });
            if enhanced {
                return Some(label);
            }
        }
    }
    None
}

/// Does this `SWITCH_LABEL` use Java 21 pattern matching for switch (a pattern, a `when` guard,
/// `case null`, or `case ..., default`)?
fn is_pattern_switch_label(label: &SyntaxNode) -> bool {
    let mut is_case = false;
    for child in label.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => match t.kind() {
                CASE_KW => is_case = true,
                DEFAULT_KW if is_case => return true,
                _ => {}
            },
            NodeOrToken::Node(n) => match n.kind() {
                TYPE_PATTERN | RECORD_PATTERN | UNNAMED_PATTERN | GUARD => return true,
                LITERAL
                    if n.children_with_tokens()
                        .any(|e| e.kind() == NULL_KW) =>
                {
                    return true;
                }
                _ => {}
            },
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_clamps_below_floor() {
        assert_eq!(Release::from_declared(5), (Release::JAVA_8, true));
        assert_eq!(Release::from_declared(7), (Release::JAVA_8, true));
        assert_eq!(Release::from_declared(8), (Release::JAVA_8, false));
        assert_eq!(Release::from_declared(21), (Release::JAVA_21, false));
        assert_eq!("1.7".parse::<Release>().unwrap(), Release::JAVA_8);
        assert_eq!("1.8".parse::<Release>().unwrap(), Release::JAVA_8);
        assert_eq!("21".parse::<Release>().unwrap(), Release::JAVA_21);
    }

    #[test]
    fn feature_table_ids_are_unique_and_ordered() {
        for (i, f) in FEATURES.iter().enumerate() {
            assert_eq!(Feature::by_id(f.id), Some(f));
            for other in &FEATURES[i + 1..] {
                assert_ne!(f.id, other.id);
            }
            if let Some(stable) = f.stable_in {
                assert!(stable > Release::FLOOR);
                for &preview in f.preview_in {
                    assert!(preview < stable, "{} preview >= stable", f.id);
                }
            }
        }
    }

    #[test]
    fn feature_availability_and_diagnostics_at_boundaries() {
        for f in FEATURES {
            let stable = f.stable_in.expect("all current features have a stable release");
            let before_preview = f
                .preview_in
                .first()
                .copied()
                .unwrap_or(stable)
                .number()
                - 1;
            let pre_level = LanguageLevel::new(Release::new(before_preview), false);
            assert!(!f.is_available(pre_level));
            assert_eq!(
                f.diagnostic(pre_level),
                Some(format!("{} require language level {}", f.plural, stable.number()))
            );

            for &p in f.preview_in {
                let off = LanguageLevel::new(p, false);
                let on = LanguageLevel::new(p, true);
                assert!(!f.is_available(off));
                assert!(f.is_available(on));
                assert_eq!(
                    f.diagnostic(off),
                    Some(format!(
                        "{} are a preview feature in java {}; enable preview",
                        f.plural,
                        p.number()
                    ))
                );
                assert_eq!(f.diagnostic(on), None);
            }

            let at_stable = LanguageLevel::new(stable, false);
            assert!(f.is_available(at_stable));
            assert_eq!(f.diagnostic(at_stable), None);
        }
    }
}
