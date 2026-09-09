//! How a node's parameters arrange themselves in a panel: the tab strip,
//! the subgroup sections inside a tab, and the group-to-keys mapping a tab
//! reset dispatches.
//!
//! Every rule here reads the declaration and the visibility answer and
//! returns names and orders. None of it decides a size, a colour or a
//! widget, which is what keeps one derivation serving a DOM panel and an
//! immediate-mode one.

use solarxy_graph::registry::param_spec::ParamSpec;

/// The name of the validation report's tab.
///
/// Registry groups are lowercase by convention, so a capitalized sentinel
/// cannot collide with one a node declares.
pub const VALIDATION_TAB: &str = "Validation";

/// A group name as the tab strip prints it.
#[must_use]
pub fn tab_label(group: &str) -> String {
    if group == VALIDATION_TAB {
        return group.to_string();
    }
    let mut chars = group.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// The tabs a node offers: general first, the rest in declaration order,
/// and the validation tab last when a report exists.
///
/// **A group contributes a tab only while at least one of its parameters
/// is visible.** Without that test a node whose whole group hides behind a
/// clause shows an empty tab, which the material node makes reachable:
/// switching it to reference mode hides every surface factor, and the tabs
/// holding them would otherwise stay in the strip with nothing under them.
/// Pass [`all_visible`] to keep every group.
pub fn param_tabs<F>(params: &[ParamSpec], has_report: bool, visible: F) -> Vec<String>
where
    F: Fn(&ParamSpec) -> bool,
{
    let mut names: Vec<&str> = Vec::new();
    for spec in params {
        if !visible(spec) {
            continue;
        }
        if !names.contains(&spec.group.as_str()) {
            names.push(&spec.group);
        }
    }
    let general = names.iter().filter(|g| g.eq_ignore_ascii_case("general"));
    let rest = names.iter().filter(|g| !g.eq_ignore_ascii_case("general"));
    let mut ordered: Vec<String> = general.chain(rest).map(|g| (*g).to_string()).collect();
    if has_report {
        ordered.push(VALIDATION_TAB.to_string());
    }
    ordered
}

/// The predicate that keeps every group, for a caller with no visibility
/// answer to hand. Named rather than written as a closure at each call
/// site, because "no predicate" is a deliberate mode rather than an
/// omission.
#[must_use]
pub fn all_visible(_: &ParamSpec) -> bool {
    true
}

/// One run of parameters under an optional subgroup heading.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSection<'a> {
    pub subgroup: Option<&'a str>,
    pub params: Vec<&'a ParamSpec>,
}

/// Splits a tab's parameters into subgroup runs, in declaration order.
///
/// Parameters declaring no subgroup collect under an unnamed heading, so a
/// group can mix loose rows with labelled sections and a node that uses no
/// subgroups at all renders exactly as it did before the level existed.
/// Consecutive runs sharing a name merge; a name reused after a different
/// one in between opens a second section, because declaration order is the
/// author's stated intent rather than something to normalize away.
#[must_use]
pub fn param_sections(params: &[ParamSpec]) -> Vec<ParamSection<'_>> {
    let mut sections: Vec<ParamSection<'_>> = Vec::new();
    for spec in params {
        let subgroup = spec.subgroup.as_deref();
        match sections.last_mut() {
            Some(last) if last.subgroup == subgroup => last.params.push(spec),
            _ => sections.push(ParamSection {
                subgroup,
                params: vec![spec],
            }),
        }
    }
    sections
}

/// The stored tab while the node still offers it, else the first tab.
///
/// The fallback is what makes selecting a different node type keep a
/// sensible tab instead of an empty panel.
#[must_use]
pub fn resolve_active_tab<'a>(tabs: &'a [String], stored: &str) -> Option<&'a str> {
    tabs.iter()
        .find(|t| *t == stored)
        .or_else(|| tabs.first())
        .map(String::as_str)
}

/// The parameter keys of one group.
///
/// The whole group is returned regardless of current visibility: a hidden
/// variant row still holds its stored value, and a reset that skipped it
/// would leave it stale to surprise someone later.
#[must_use]
pub fn group_keys<'a>(params: &'a [ParamSpec], group: &str) -> Vec<&'a str> {
    params
        .iter()
        .filter(|spec| spec.group == group)
        .map(|spec| spec.key.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::params::ParamValue;
    use solarxy_graph::registry::param_spec::{ParamType, Pred};
    use solarxy_graph::registry::visibility::param_visible;

    fn spec(key: &str, group: &str) -> ParamSpec {
        ParamSpec::new(key, key, group, ParamType::Float, ParamValue::Float(0.0))
    }

    fn names<'a>(sections: &'a [ParamSection<'a>]) -> Vec<(Option<&'a str>, Vec<&'a str>)> {
        sections
            .iter()
            .map(|s| {
                (
                    s.subgroup,
                    s.params.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
                )
            })
            .collect()
    }

    #[test]
    fn orders_general_first_and_appends_the_validation_tab_only_with_a_report() {
        let specs = [
            spec("a", "attribute"),
            spec("b", "general"),
            spec("c", "attribute"),
        ];
        assert_eq!(
            param_tabs(&specs, false, all_visible),
            ["general", "attribute"]
        );
        assert_eq!(
            param_tabs(&specs, true, all_visible),
            ["general", "attribute", VALIDATION_TAB]
        );
    }

    #[test]
    fn falls_back_to_the_first_tab_when_the_stored_one_is_gone() {
        let two = ["general".to_string(), "attribute".to_string()];
        let one = ["general".to_string()];
        let none: [String; 0] = [];
        assert_eq!(resolve_active_tab(&two, "attribute"), Some("attribute"));
        assert_eq!(resolve_active_tab(&one, "attribute"), Some("general"));
        assert_eq!(resolve_active_tab(&none, "attribute"), None);
    }

    #[test]
    fn maps_a_group_to_its_keys_for_the_tab_reset() {
        let specs = [
            spec("type", "attribute"),
            spec("value_float", "attribute"),
            spec("value_vec3", "attribute"),
        ];
        assert_eq!(
            group_keys(&specs, "attribute"),
            ["type", "value_float", "value_vec3"]
        );
        assert!(group_keys(&specs, "nope").is_empty());
    }

    #[test]
    fn keeps_a_node_that_declares_no_subgroups_as_one_unlabelled_run() {
        let specs = [spec("a", "general"), spec("b", "general")];
        assert_eq!(names(&param_sections(&specs)), [(None, vec!["a", "b"])]);
    }

    #[test]
    fn splits_consecutive_runs_and_merges_neighbours_sharing_a_name() {
        let specs = [
            spec("a", "general"),
            spec("b", "general").subgroup("Clearcoat"),
            spec("c", "general").subgroup("Clearcoat"),
            spec("d", "general").subgroup("Sheen"),
        ];
        assert_eq!(
            names(&param_sections(&specs)),
            [
                (None, vec!["a"]),
                (Some("Clearcoat"), vec!["b", "c"]),
                (Some("Sheen"), vec!["d"]),
            ]
        );
    }

    #[test]
    fn reopens_a_section_when_a_name_returns_after_a_different_one() {
        // Declaration order is the author's stated intent, so a name
        // coming back is a second section rather than a reason to reorder
        // the panel.
        let specs = [
            spec("a", "general").subgroup("One"),
            spec("b", "general").subgroup("Two"),
            spec("c", "general").subgroup("One"),
        ];
        let got: Vec<Option<&str>> = param_sections(&specs).iter().map(|s| s.subgroup).collect();
        assert_eq!(got, [Some("One"), Some("Two"), Some("One")]);
    }

    #[test]
    fn drops_a_tab_whose_every_param_is_hidden() {
        // Reachable on the material node: switching it to reference mode
        // hides every surface factor, and the tabs holding them would
        // otherwise stay in the strip with nothing under them.
        let specs = vec![
            ParamSpec::new(
                "mode",
                "Mode",
                "base",
                ParamType::Enum { variants: vec![] },
                ParamValue::Enum("inline".into()),
            ),
            spec("clearcoat", "surface")
                .show_if("mode", Pred::Eq(ParamValue::Enum("inline".into()))),
        ];
        let tabs_in = |mode: &str| {
            let params = std::iter::once((
                "mode".to_string(),
                solarxy_graph::params::ParamSource::Literal(ParamValue::Enum(mode.into())),
            ))
            .collect();
            param_tabs(&specs, false, |p| param_visible(p, &specs, &params))
        };
        assert_eq!(tabs_in("inline"), ["base", "surface"]);
        assert_eq!(tabs_in("reference"), ["base"]);
        // No predicate keeps every group, which is the pre-existing
        // behaviour and the reason `all_visible` has a name.
        assert_eq!(param_tabs(&specs, false, all_visible), ["base", "surface"]);
    }

    #[test]
    fn a_tab_label_capitalizes_the_group_and_leaves_the_sentinel_alone() {
        assert_eq!(tab_label("general"), "General");
        assert_eq!(tab_label("attribute"), "Attribute");
        assert_eq!(tab_label(VALIDATION_TAB), VALIDATION_TAB);
        assert_eq!(tab_label(""), "");
    }
}
