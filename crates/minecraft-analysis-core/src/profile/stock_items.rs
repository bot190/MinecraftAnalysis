//! Version-specific stock identity membership, independent of recovery catalogs.
//!
//! The Forge 1.2.5 set is the baseline. Each later profile applies sorted
//! additions and removals relative to the preceding supported version. This
//! keeps historical changes explicit without duplicating every surviving name.
//!
//! The data is transcribed from the registered item bootstraps, not creative
//! tabs or the block registry alone:
//! <https://github.com/Bukkit/mc-dev/blob/c1627dc9cc7505581993eb0fa15597cb36e94244/net/minecraft/server/Item.java>
//! <https://github.com/KealJones/mc-1.12.2-source_files/blob/12ae74ca50912e7f3fe279eeabd3ec02526856e6/src/minecraft/net/minecraft/item/Item.java>
//!
//! Every slice is sorted and unique so membership uses binary search.

use super::WorldProfile;

mod base_1_2_5;
mod changes_1_12_2;
mod changes_1_7_10;

fn listed(items: &[&str], name: &str) -> bool {
    items.binary_search(&name).is_ok()
}

fn in_1_7_10(name: &str) -> bool {
    (listed(base_1_2_5::ITEMS, name) && !listed(changes_1_7_10::REMOVED, name))
        || listed(changes_1_7_10::ADDED, name)
}

pub(super) fn contains(profile: WorldProfile, name: &str) -> bool {
    match profile {
        WorldProfile::Forge1_2_5 => listed(base_1_2_5::ITEMS, name),
        WorldProfile::Forge1_7_10 => in_1_7_10(name),
        WorldProfile::Forge1_12_2 => {
            (in_1_7_10(name) && !listed(changes_1_12_2::REMOVED, name))
                || listed(changes_1_12_2::ADDED, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn candidates() -> impl Iterator<Item = &'static str> {
        base_1_2_5::ITEMS
            .iter()
            .chain(changes_1_7_10::ADDED)
            .chain(changes_1_7_10::REMOVED)
            .chain(changes_1_12_2::ADDED)
            .chain(changes_1_12_2::REMOVED)
            .copied()
    }

    fn names(profile: WorldProfile) -> BTreeSet<&'static str> {
        candidates()
            .filter(|name| contains(profile, name))
            .collect()
    }

    #[test]
    fn deltas_reconstruct_complete_sorted_version_sets() {
        for items in [
            base_1_2_5::ITEMS,
            changes_1_7_10::ADDED,
            changes_1_7_10::REMOVED,
            changes_1_12_2::ADDED,
            changes_1_12_2::REMOVED,
        ] {
            assert!(items.windows(2).all(|pair| pair[0] < pair[1]));
        }

        let one_two_five = names(WorldProfile::Forge1_2_5);
        let one_seven_ten = names(WorldProfile::Forge1_7_10);
        let one_twelve_two = names(WorldProfile::Forge1_12_2);
        assert_eq!(changes_1_7_10::ADDED.len(), 73);
        assert_eq!(changes_1_7_10::REMOVED.len(), 14);
        assert_eq!(changes_1_12_2::ADDED.len(), 110);
        assert_eq!(changes_1_12_2::REMOVED.len(), 14);
        assert_eq!(one_two_five.len(), 256);
        assert_eq!(one_seven_ten.len(), 315);
        assert_eq!(one_twelve_two.len(), 411);

        assert_eq!(
            one_seven_ten
                .difference(&one_two_five)
                .copied()
                .collect::<Vec<_>>(),
            changes_1_7_10::ADDED
        );
        assert_eq!(
            one_two_five
                .difference(&one_seven_ten)
                .copied()
                .collect::<Vec<_>>(),
            changes_1_7_10::REMOVED
        );
        assert_eq!(
            one_twelve_two
                .difference(&one_seven_ten)
                .copied()
                .collect::<Vec<_>>(),
            changes_1_12_2::ADDED
        );
        assert_eq!(
            one_seven_ten
                .difference(&one_twelve_two)
                .copied()
                .collect::<Vec<_>>(),
            changes_1_12_2::REMOVED
        );
    }
}
