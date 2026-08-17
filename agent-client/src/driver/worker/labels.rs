//! Sell/drop labels, parsed from the app-managed block at the foot of a
//! character's instance prompt. Rule workers sell and drop only what these
//! labels name, so the marks a player applies in the bag drawer survive
//! rule-based play.

/// The sellable/dropable lists named in a character's instance prompt.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct BagLabels {
    pub sellable: Vec<String>,
    pub dropable: Vec<String>,
}

/// The marker the desktop app writes above its labels block.
const LABELS_MARKER: &str = "<!-- BAG LABELS: DO NOT EDIT BELOW (managed by the app) -->";

/// Load the labels from a character's instance prompt. A missing file or
/// block reads as no labels: nothing sells, nothing drops.
pub(crate) fn labels_from_prompt(path: Option<&str>) -> BagLabels {
    let Some(path) = path else {
        return BagLabels::default();
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse_labels(&text),
        Err(_) => BagLabels::default(),
    }
}

/// Parse the `Sellable:`/`Dropable:` lines below the app's marker. Prose
/// above the marker is the player's own and never mistaken for a label.
fn parse_labels(text: &str) -> BagLabels {
    let Some(at) = text.lines().position(|l| l.contains(LABELS_MARKER)) else {
        return BagLabels::default();
    };
    let mut labels = BagLabels::default();
    for line in text.lines().skip(at + 1) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Sellable:") {
            labels.sellable = split_ids(rest);
        } else if let Some(rest) = line.strip_prefix("Dropable:") {
            labels.dropable = split_ids(rest);
        }
    }
    labels
}

fn split_ids(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_app_managed_labels_block() {
        let text = "You are a fisher.\n\n<!-- BAG LABELS: DO NOT EDIT BELOW (managed by the app) -->\nSellable: gold_ring, iron_sword\nDropable: old_boot\n";
        let labels = parse_labels(text);
        assert_eq!(labels.sellable, ["gold_ring", "iron_sword"]);
        assert_eq!(labels.dropable, ["old_boot"]);
    }

    #[test]
    fn prose_before_the_marker_is_not_a_label() {
        let text = "Sellable: everything\n\nNo label block.\n";
        assert_eq!(parse_labels(text), BagLabels::default());
    }

    #[test]
    fn a_missing_file_reads_as_no_labels() {
        assert_eq!(labels_from_prompt(None), BagLabels::default());
        assert_eq!(labels_from_prompt(Some("no/such/file.txt")), BagLabels::default());
    }
}
