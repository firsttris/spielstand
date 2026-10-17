//! The full plan for one game on one device: first recognise a disk that holds a version the
//! server already knows (another sync tool may have copied it there), then decide by mode.
//!
//! - **Sync**: the four cases of [`decide`](crate::decide::decide); Spielstand writes saves.
//! - **Backup**: Spielstand never writes on its own; it only records what is on the disk.
//!
//! A disk that went from the device's base to a version that does not descend from it lost
//! the base's progress on this device: some other tool (Stellwerk, Syncthing, a copy by hand)
//! replaced newer saves with older or different ones. That is reported as `overwritten`.

use crate::decide::{self, Decision, Local, Remote};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SyncMode {
    /// Spielstand moves saves between devices.
    #[default]
    Sync,
    /// Spielstand only backs up; something else (or nobody) moves the saves.
    Backup,
}

/// The server's answer to "which version has this content?" for the disk's content hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Known<'a> {
    pub version: &'a str,
    /// `version` has the device's base among its ancestors (or the device has no base yet).
    pub descends_from_base: bool,
    /// `version` is a branch whose conflict is still open.
    pub is_open_branch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The disk holds this known version: record it as the base before anything else.
    pub adopt: Option<String>,
    /// The base this device held before the disk was replaced by an older or unrelated version.
    pub overwritten: Option<String>,
    pub decision: Decision,
}

pub fn plan(local: Local<'_>, known: Option<Known<'_>>, remote: Remote<'_>, mode: SyncMode) -> Plan {
    let changed = local.disk_hash.is_some() && local.disk_hash != local.base_hash;
    let recognised = known.filter(|k| changed && Some(k.version) != local.base);

    let (local, adopt, overwritten) = match recognised {
        Some(k) => {
            let overwritten = (!k.descends_from_base)
                .then(|| local.base.map(str::to_string))
                .flatten();
            let local = Local {
                base: Some(k.version),
                base_hash: local.disk_hash,
                disk_hash: local.disk_hash,
                base_is_open_branch: k.is_open_branch,
            };
            (local, Some(k.version.to_string()), overwritten)
        }
        None => (local, None, None),
    };

    let decision = match mode {
        SyncMode::Sync => decide::decide(local, remote),
        SyncMode::Backup => backup(local, remote),
    };
    // An adoption already records the base; a second `Adopt` of the same version adds nothing.
    let decision = match decision {
        Decision::Adopt { version } if adopt.as_deref() == Some(version.as_str()) => Decision::Nothing,
        other => other,
    };
    Plan {
        adopt,
        overwritten,
        decision,
    }
}

/// Backup mode: record what the disk holds, never write to it.
fn backup(local: Local<'_>, remote: Remote<'_>) -> Decision {
    if let (Some(disk), Some(head)) = (local.disk_hash, remote.head)
        && Some(disk) == remote.head_hash
        && local.base != Some(head)
    {
        return Decision::Adopt {
            version: head.to_string(),
        };
    }
    if local.disk_hash.is_some() && local.disk_hash != local.base_hash {
        Decision::Upload
    } else {
        Decision::Nothing
    }
}

/// Decisions that write saves to the disk.
pub fn writes(decision: &Decision) -> bool {
    matches!(decision, Decision::FastForward { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn local<'a>(base: Option<&'a str>, base_hash: Option<&'a str>, disk: Option<&'a str>) -> Local<'a> {
        Local {
            base,
            base_hash,
            disk_hash: disk,
            base_is_open_branch: false,
        }
    }

    fn remote<'a>(head: &'a str, hash: &'a str) -> Remote<'a> {
        Remote {
            head: Some(head),
            head_hash: Some(hash),
        }
    }

    fn known(version: &str, descends: bool) -> Known<'_> {
        Known {
            version,
            descends_from_base: descends,
            is_open_branch: false,
        }
    }

    #[test]
    fn a_newer_known_version_brought_by_another_tool_is_adopted() {
        // Stellwerk copied v6 from the other PC; v6 descends from this PC's v5.
        let p = plan(
            local(Some("v5"), Some("h5"), Some("h6")),
            Some(known("v6", true)),
            remote("v6", "h6"),
            SyncMode::Backup,
        );
        assert_eq!(p.adopt.as_deref(), Some("v6"));
        assert_eq!(p.overwritten, None);
        assert_eq!(p.decision, Decision::Nothing);
    }

    #[test]
    fn an_older_version_over_a_newer_one_is_reported() {
        let p = plan(
            local(Some("v6"), Some("h6"), Some("h5")),
            Some(known("v5", false)),
            remote("v6", "h6"),
            SyncMode::Backup,
        );
        assert_eq!(p.adopt.as_deref(), Some("v5"));
        assert_eq!(p.overwritten.as_deref(), Some("v6"));
        assert_eq!(p.decision, Decision::Nothing, "backup mode never writes");
    }

    #[test]
    fn sync_mode_heals_an_overwrite_by_loading_the_head_again() {
        let p = plan(
            local(Some("v6"), Some("h6"), Some("h5")),
            Some(known("v5", false)),
            remote("v6", "h6"),
            SyncMode::Sync,
        );
        assert_eq!(p.overwritten.as_deref(), Some("v6"));
        assert_eq!(p.decision, Decision::FastForward { version: "v6".into() });
    }

    #[test]
    fn unknown_content_is_uploaded_in_both_modes() {
        for mode in [SyncMode::Sync, SyncMode::Backup] {
            let p = plan(
                local(Some("v5"), Some("h5"), Some("hX")),
                None,
                remote("v5", "h5"),
                mode,
            );
            assert_eq!(p.decision, Decision::Upload);
            assert_eq!(p.adopt, None);
        }
    }

    #[test]
    fn backup_mode_uploads_where_sync_mode_would_conflict() {
        let p = plan(
            local(Some("v4"), Some("h4"), Some("hX")),
            None,
            remote("v5", "h5"),
            SyncMode::Backup,
        );
        assert_eq!(p.decision, Decision::Upload);
    }

    #[test]
    fn backup_mode_does_not_fast_forward() {
        let p = plan(
            local(Some("v4"), Some("h4"), Some("h4")),
            None,
            remote("v5", "h5"),
            SyncMode::Backup,
        );
        assert_eq!(p.decision, Decision::Nothing);
    }

    #[test]
    fn a_new_device_with_a_copied_save_just_adopts_it() {
        let p = plan(
            local(None, None, Some("h5")),
            Some(known("v5", true)),
            remote("v5", "h5"),
            SyncMode::Sync,
        );
        assert_eq!(p.adopt.as_deref(), Some("v5"));
        assert_eq!(p.overwritten, None);
        assert_eq!(p.decision, Decision::Nothing);
    }

    #[test]
    fn an_adopted_open_branch_is_not_fast_forwarded() {
        let p = plan(
            local(Some("v5"), Some("h5"), Some("h6")),
            Some(Known {
                version: "b6",
                descends_from_base: true,
                is_open_branch: true,
            }),
            remote("v7", "h7"),
            SyncMode::Sync,
        );
        assert_eq!(p.decision, Decision::Conflict { head: "v7".into() });
    }

    fn mode() -> impl Strategy<Value = SyncMode> {
        prop_oneof![Just(SyncMode::Sync), Just(SyncMode::Backup)]
    }

    proptest! {
        /// Backup mode never plans a write, whatever the disk, base and server look like.
        #[test]
        fn backup_mode_never_writes(
            base in proptest::option::of(0u8..4),
            disk in proptest::option::of(0u8..4),
            head in proptest::option::of(0u8..4),
            known_v in proptest::option::of((0u8..4, any::<bool>(), any::<bool>())),
        ) {
            let id = |n: u8| format!("v{n}");
            let hash = |n: u8| format!("h{n}");
            let (bi, bh) = (base.map(id), base.map(hash));
            let dh = disk.map(hash);
            let (hi, hh) = (head.map(id), head.map(hash));
            let kv = known_v.map(|(n, d, o)| (id(n), d, o));
            let p = plan(
                Local { base: bi.as_deref(), base_hash: bh.as_deref(), disk_hash: dh.as_deref(), base_is_open_branch: false },
                kv.as_ref().map(|(v, d, o)| Known { version: v, descends_from_base: *d, is_open_branch: *o }),
                Remote { head: hi.as_deref(), head_hash: hh.as_deref() },
                SyncMode::Backup,
            );
            prop_assert!(!writes(&p.decision));
        }

        /// An overwrite is only ever reported for a device that had a base.
        #[test]
        fn overwrite_needs_a_base(
            m in mode(),
            disk in 0u8..4,
            descends in any::<bool>(),
        ) {
            let dh = format!("h{disk}");
            let p = plan(
                Local { base: None, base_hash: None, disk_hash: Some(&dh), base_is_open_branch: false },
                Some(Known { version: "vK", descends_from_base: descends, is_open_branch: false }),
                Remote::default(),
                m,
            );
            prop_assert_eq!(p.overwritten, None);
        }
    }
}
