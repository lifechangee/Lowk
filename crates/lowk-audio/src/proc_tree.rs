//! Platform-neutral process-tree helper (unit-tested on every OS; used by the Windows backend).

/// Root of an app's process tree: walk up while the parent runs the same executable name.
/// `lookup(pid)` returns `(parent pid, exe name)` or `None` for an unknown / dead process.
/// A guard bounds the walk so a pid-reuse cycle can never loop forever.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn tree_root(pid: u32, lookup: impl Fn(u32) -> Option<(Option<u32>, String)>) -> u32 {
    let mut cur = pid;
    for _ in 0..64 {
        let Some((Some(parent), name)) = lookup(cur) else {
            break;
        };
        match lookup(parent) {
            Some((_, pname)) if pname.eq_ignore_ascii_case(&name) && parent != cur => cur = parent,
            _ => break,
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn table(rows: &[(u32, Option<u32>, &str)]) -> impl Fn(u32) -> Option<(Option<u32>, String)> {
        let m: HashMap<u32, (Option<u32>, String)> =
            rows.iter().map(|(p, pp, n)| (*p, (*pp, n.to_string()))).collect();
        move |p| m.get(&p).cloned()
    }

    #[test]
    fn walks_to_the_root_of_same_named_processes() {
        let t = table(&[
            (10, Some(1), "explorer.exe"),
            (20, Some(10), "Tolk.exe"),
            (30, Some(20), "tolk.exe"),
            (40, Some(30), "Tolk.exe"),
        ]);
        assert_eq!(tree_root(40, &t), 20);
        assert_eq!(tree_root(20, &t), 20);
    }

    #[test]
    fn stops_at_a_different_parent_or_unknown_process() {
        let t = table(&[
            (20, Some(10), "Tolk.exe"),
            (10, None, "launcher.exe"),
            (50, Some(99), "x.exe"),
        ]);
        assert_eq!(tree_root(20, &t), 20);
        assert_eq!(tree_root(50, &t), 50);
        assert_eq!(tree_root(777, &t), 777);
    }

    #[test]
    fn a_pid_cycle_terminates() {
        let t = table(&[(1, Some(2), "a.exe"), (2, Some(1), "a.exe")]);
        let _ = tree_root(1, &t);
    }
}
