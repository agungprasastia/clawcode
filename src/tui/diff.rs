const CONTEXT_LINES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    Same,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub op: DiffOp,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub added: usize,
    pub removed: usize,
    pub lines: Vec<DiffLine>,
}

fn lcs_diff(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffLine> {
    let mut start = 0;
    while start < old_lines.len() && start < new_lines.len() && old_lines[start] == new_lines[start] {
        start += 1;
    }

    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > start && new_end > start && old_lines[old_end - 1] == new_lines[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    let mut result = Vec::new();

    for &line in &old_lines[..start] {
        result.push(DiffLine {
            op: DiffOp::Same,
            text: line.to_string(),
        });
    }

    let mid_old = &old_lines[start..old_end];
    let mid_new = &new_lines[start..new_end];

    if mid_old.is_empty() {
        for &line in mid_new {
            result.push(DiffLine {
                op: DiffOp::Add,
                text: line.to_string(),
            });
        }
    } else if mid_new.is_empty() {
        for &line in mid_old {
            result.push(DiffLine {
                op: DiffOp::Remove,
                text: line.to_string(),
            });
        }
    } else {
        let n = mid_old.len();
        let m = mid_new.len();

        if n * m > 250_000 {
            for &line in mid_old {
                result.push(DiffLine {
                    op: DiffOp::Remove,
                    text: line.to_string(),
                });
            }
            for &line in mid_new {
                result.push(DiffLine {
                    op: DiffOp::Add,
                    text: line.to_string(),
                });
            }
        } else {
            let mut dp = vec![vec![0u32; m + 1]; n + 1];
            for i in 0..n {
                for j in 0..m {
                    if mid_old[i] == mid_new[j] {
                        dp[i + 1][j + 1] = dp[i][j] + 1;
                    } else {
                        dp[i + 1][j + 1] = dp[i + 1][j].max(dp[i][j + 1]);
                    }
                }
            }

            let mut i = n;
            let mut j = m;
            let mut mid_ops = Vec::new();
            while i > 0 || j > 0 {
                if i > 0 && j > 0 && mid_old[i - 1] == mid_new[j - 1] {
                    mid_ops.push(DiffLine {
                        op: DiffOp::Same,
                        text: mid_old[i - 1].to_string(),
                    });
                    i -= 1;
                    j -= 1;
                } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
                    mid_ops.push(DiffLine {
                        op: DiffOp::Add,
                        text: mid_new[j - 1].to_string(),
                    });
                    j -= 1;
                } else if i > 0 {
                    mid_ops.push(DiffLine {
                        op: DiffOp::Remove,
                        text: mid_old[i - 1].to_string(),
                    });
                    i -= 1;
                }
            }
            mid_ops.reverse();
            result.extend(mid_ops);
        }
    }

    for &line in &old_lines[old_end..] {
        result.push(DiffLine {
            op: DiffOp::Same,
            text: line.to_string(),
        });
    }

    result
}

pub fn compute_diff(old_text: &str, new_text: &str, max_lines: usize) -> DiffResult {
    let old_lines: Vec<&str> = if old_text.is_empty() {
        Vec::new()
    } else {
        old_text.lines().collect()
    };
    let new_lines: Vec<&str> = if new_text.is_empty() {
        Vec::new()
    } else {
        new_text.lines().collect()
    };

    let all_lines = lcs_diff(&old_lines, &new_lines);

    let added = all_lines.iter().filter(|l| l.op == DiffOp::Add).count();
    let removed = all_lines.iter().filter(|l| l.op == DiffOp::Remove).count();

    if max_lines == 0 {
        return DiffResult {
            added,
            removed,
            lines: Vec::new(),
        };
    }

    if all_lines.len() <= max_lines {
        return DiffResult {
            added,
            removed,
            lines: all_lines,
        };
    }

    let change_indices: Vec<usize> = all_lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.op != DiffOp::Same)
        .map(|(i, _)| i)
        .collect();

    if change_indices.is_empty() {
        let mut lines = all_lines;
        lines.truncate(max_lines);
        return DiffResult {
            added,
            removed,
            lines,
        };
    }

    let mut keep = vec![false; all_lines.len()];
    for &idx in &change_indices {
        let start = idx.saturating_sub(CONTEXT_LINES);
        let end = (idx + CONTEXT_LINES + 1).min(all_lines.len());
        for i in start..end {
            keep[i] = true;
        }
    }

    let mut filtered = Vec::new();
    let mut in_ellipsis = false;
    for (i, line) in all_lines.into_iter().enumerate() {
        if keep[i] {
            filtered.push(line);
            in_ellipsis = false;
        } else if !in_ellipsis {
            filtered.push(DiffLine {
                op: DiffOp::Same,
                text: "⋯".to_string(),
            });
            in_ellipsis = true;
        }
    }

    if filtered.len() > max_lines {
        filtered.truncate(max_lines);
    }

    DiffResult {
        added,
        removed,
        lines: filtered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_diff_same_text() {
        let text = "line 1\nline 2\nline 3";
        let res = compute_diff(text, text, 20);
        assert_eq!(res.added, 0);
        assert_eq!(res.removed, 0);
        assert_eq!(res.lines.len(), 3);
        assert!(res.lines.iter().all(|l| l.op == DiffOp::Same));
    }

    #[test]
    fn test_compute_diff_added_text() {
        let old = "line 1\nline 3";
        let new = "line 1\nline 2\nline 3";
        let res = compute_diff(old, new, 20);
        assert_eq!(res.added, 1);
        assert_eq!(res.removed, 0);
        assert_eq!(res.lines.len(), 3);
        assert_eq!(res.lines[1].op, DiffOp::Add);
        assert_eq!(res.lines[1].text, "line 2");
    }

    #[test]
    fn test_compute_diff_removed_text() {
        let old = "line 1\nline 2\nline 3";
        let new = "line 1\nline 3";
        let res = compute_diff(old, new, 20);
        assert_eq!(res.added, 0);
        assert_eq!(res.removed, 1);
        assert_eq!(res.lines.len(), 3);
        assert_eq!(res.lines[1].op, DiffOp::Remove);
        assert_eq!(res.lines[1].text, "line 2");
    }

    #[test]
    fn test_compute_diff_middle_modification() {
        let old = "alpha\nbeta\ngamma\ndelta";
        let new = "alpha\nBETA_MODIFIED\ngamma\ndelta";
        let res = compute_diff(old, new, 20);
        assert_eq!(res.added, 1);
        assert_eq!(res.removed, 1);
        assert_eq!(res.lines.len(), 5);
        assert_eq!(res.lines[0], DiffLine { op: DiffOp::Same, text: "alpha".to_string() });
        assert_eq!(res.lines[1], DiffLine { op: DiffOp::Remove, text: "beta".to_string() });
        assert_eq!(res.lines[2], DiffLine { op: DiffOp::Add, text: "BETA_MODIFIED".to_string() });
        assert_eq!(res.lines[3], DiffLine { op: DiffOp::Same, text: "gamma".to_string() });
        assert_eq!(res.lines[4], DiffLine { op: DiffOp::Same, text: "delta".to_string() });
    }

    #[test]
    fn test_compute_diff_max_lines_limit() {
        let old_lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let mut new_lines = old_lines.clone();
        new_lines[25] = "changed line 25".to_string();

        let old = old_lines.join("\n");
        let new = new_lines.join("\n");

        let res = compute_diff(&old, &new, 5);
        assert_eq!(res.added, 1);
        assert_eq!(res.removed, 1);
        assert!(res.lines.len() <= 5);
    }
}
