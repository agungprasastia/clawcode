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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideRow {
    pub left_num: Option<usize>,
    pub left_sign: Option<char>,
    pub left_text: String,
    pub right_num: Option<usize>,
    pub right_sign: Option<char>,
    pub right_text: String,
}

pub fn compute_side_by_side_diff(
    old_text: &str,
    new_text: &str,
    start_line: usize,
    max_lines: usize,
) -> Vec<SideBySideRow> {
    if max_lines == 0 {
        return Vec::new();
    }

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

    if old_lines.is_empty() && new_lines.is_empty() {
        return Vec::new();
    }

    let diff_lines = lcs_diff(&old_lines, &new_lines);

    let mut all_rows = Vec::new();
    let mut old_line_no = start_line;
    let mut new_line_no = start_line;

    let mut i = 0;
    while i < diff_lines.len() {
        match diff_lines[i].op {
            DiffOp::Same => {
                all_rows.push(SideBySideRow {
                    left_num: Some(old_line_no),
                    left_sign: None,
                    left_text: diff_lines[i].text.clone(),
                    right_num: Some(new_line_no),
                    right_sign: None,
                    right_text: diff_lines[i].text.clone(),
                });
                old_line_no += 1;
                new_line_no += 1;
                i += 1;
            }
            DiffOp::Remove | DiffOp::Add => {
                let mut removes = Vec::new();
                let mut adds = Vec::new();

                while i < diff_lines.len() && diff_lines[i].op != DiffOp::Same {
                    if diff_lines[i].op == DiffOp::Remove {
                        removes.push((old_line_no, diff_lines[i].text.clone()));
                        old_line_no += 1;
                    } else if diff_lines[i].op == DiffOp::Add {
                        adds.push((new_line_no, diff_lines[i].text.clone()));
                        new_line_no += 1;
                    }
                    i += 1;
                }

                let count = removes.len().max(adds.len());
                for k in 0..count {
                    let (left_num, left_sign, left_text) = if k < removes.len() {
                        (Some(removes[k].0), Some('-'), removes[k].1.clone())
                    } else {
                        (None, None, String::new())
                    };
                    let (right_num, right_sign, right_text) = if k < adds.len() {
                        (Some(adds[k].0), Some('+'), adds[k].1.clone())
                    } else {
                        (None, None, String::new())
                    };
                    all_rows.push(SideBySideRow {
                        left_num,
                        left_sign,
                        left_text,
                        right_num,
                        right_sign,
                        right_text,
                    });
                }
            }
        }
    }

    if all_rows.len() <= max_lines {
        return all_rows;
    }

    let change_indices: Vec<usize> = all_rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.left_sign.is_some() || r.right_sign.is_some())
        .map(|(idx, _)| idx)
        .collect();

    if change_indices.is_empty() {
        let mut rows = all_rows;
        rows.truncate(max_lines);
        return rows;
    }

    let mut keep = vec![false; all_rows.len()];
    for &idx in &change_indices {
        let start = idx.saturating_sub(CONTEXT_LINES);
        let end = (idx + CONTEXT_LINES + 1).min(all_rows.len());
        keep[start..end].fill(true);
    }

    let mut filtered = Vec::new();
    let mut in_ellipsis = false;
    for (idx, row) in all_rows.into_iter().enumerate() {
        if keep[idx] {
            filtered.push(row);
            in_ellipsis = false;
        } else if !in_ellipsis {
            filtered.push(SideBySideRow {
                left_num: None,
                left_sign: None,
                left_text: "⋯".to_string(),
                right_num: None,
                right_sign: None,
                right_text: "⋯".to_string(),
            });
            in_ellipsis = true;
        }
    }

    if filtered.len() > max_lines {
        filtered.truncate(max_lines);
    }

    filtered
}

fn format_col(num: Option<usize>, sign: Option<char>, text: &str, col_width: usize) -> String {
    if col_width == 0 {
        return String::new();
    }
    if num.is_none() && sign.is_none() && text.is_empty() {
        return " ".repeat(col_width);
    }
    let num_str = match num {
        Some(n) => format!("{n:>4}"),
        None => "    ".to_string(),
    };
    let sign_char = sign.unwrap_or(' ');
    let gutter = format!("{num_str} {sign_char} ");
    if col_width <= gutter.len() {
        let col: String = gutter.chars().take(col_width).collect();
        return col;
    }
    let max_text_len = col_width.saturating_sub(gutter.len());
    let truncated_text = if text.chars().count() > max_text_len {
        if max_text_len >= 3 {
            let take_len = max_text_len.saturating_sub(3);
            let mut s: String = text.chars().take(take_len).collect();
            s.push_str("...");
            s
        } else {
            text.chars().take(max_text_len).collect()
        }
    } else {
        text.to_string()
    };
    let mut col = format!("{gutter}{truncated_text}");
    let count = col.chars().count();
    if count < col_width {
        col.push_str(&" ".repeat(col_width - count));
    }
    col
}

pub fn format_side_by_side_diff(rows: &[SideBySideRow], col_width: usize) -> String {
    let width = if col_width == 0 { 40 } else { col_width };
    let mut out = String::new();
    for row in rows {
        let left_col = format_col(row.left_num, row.left_sign, &row.left_text, width);
        let right_col = format_col(row.right_num, row.right_sign, &row.right_text, width);
        out.push_str(&format!("    {left_col} │ {right_col}\n"));
    }
    out
}

fn lcs_diff(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffLine> {
    let mut start = 0;
    while start < old_lines.len() && start < new_lines.len() && old_lines[start] == new_lines[start]
    {
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

        if n.checked_mul(m).is_none_or(|prod| prod > 250_000) {
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
        keep[start..end].fill(true);
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
        assert_eq!(
            res.lines[0],
            DiffLine {
                op: DiffOp::Same,
                text: "alpha".to_string()
            }
        );
        assert_eq!(
            res.lines[1],
            DiffLine {
                op: DiffOp::Remove,
                text: "beta".to_string()
            }
        );
        assert_eq!(
            res.lines[2],
            DiffLine {
                op: DiffOp::Add,
                text: "BETA_MODIFIED".to_string()
            }
        );
        assert_eq!(
            res.lines[3],
            DiffLine {
                op: DiffOp::Same,
                text: "gamma".to_string()
            }
        );
        assert_eq!(
            res.lines[4],
            DiffLine {
                op: DiffOp::Same,
                text: "delta".to_string()
            }
        );
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

    #[test]
    fn test_compute_side_by_side_diff_replacement() {
        let old = "public function up(): void\n{\n    //\n}";
        let new = "public function up(): void\n{\n    $table->date('dob');\n}";
        let rows = compute_side_by_side_diff(old, new, 12, 20);
        assert_eq!(rows.len(), 4);

        // Row 0: Same
        assert_eq!(rows[0].left_num, Some(12));
        assert_eq!(rows[0].left_sign, None);
        assert_eq!(rows[0].right_num, Some(12));
        assert_eq!(rows[0].right_sign, None);

        // Row 2: replacement (left has -, right has +)
        assert_eq!(rows[2].left_num, Some(14));
        assert_eq!(rows[2].left_sign, Some('-'));
        assert_eq!(rows[2].left_text, "    //");
        assert_eq!(rows[2].right_num, Some(14));
        assert_eq!(rows[2].right_sign, Some('+'));
        assert_eq!(rows[2].right_text, "    $table->date('dob');");
    }

    #[test]
    fn test_compute_side_by_side_diff_add_and_remove() {
        let old = "line 1\nline 2";
        let new = "line 1\nline 2\nline 3";
        let rows = compute_side_by_side_diff(old, new, 1, 20);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].left_num, None);
        assert_eq!(rows[2].left_sign, None);
        assert_eq!(rows[2].right_num, Some(3));
        assert_eq!(rows[2].right_sign, Some('+'));
        assert_eq!(rows[2].right_text, "line 3");
    }

    #[test]
    fn test_format_side_by_side_diff() {
        let rows = vec![SideBySideRow {
            left_num: Some(15),
            left_sign: Some('-'),
            left_text: "//".to_string(),
            right_num: Some(15),
            right_sign: Some('+'),
            right_text: "$table->date('dob');".to_string(),
        }];
        let formatted = format_side_by_side_diff(&rows, 30);
        assert!(formatted.contains(" │ "));
        assert!(formatted.contains("  15 - //"));
        assert!(formatted.contains("  15 + $table->date('dob');"));
    }

    #[test]
    fn test_diff_edge_cases() {
        // Empty inputs
        let res = compute_diff("", "", 10);
        assert_eq!(res.added, 0);
        assert_eq!(res.removed, 0);
        assert!(res.lines.is_empty());

        // Single character inputs
        let res = compute_diff("a", "b", 10);
        assert_eq!(res.added, 1);
        assert_eq!(res.removed, 1);

        // Identical inputs
        let res = compute_diff("same\ntext", "same\ntext", 10);
        assert_eq!(res.added, 0);
        assert_eq!(res.removed, 0);

        // Side-by-side with small col_width (narrow terminal)
        let rows = vec![SideBySideRow {
            left_num: Some(1),
            left_sign: Some('-'),
            left_text: "old line of text".to_string(),
            right_num: Some(1),
            right_sign: Some('+'),
            right_text: "new line of text".to_string(),
        }];
        for width in [0, 1, 5, 8, 10, 20, 30] {
            let formatted = format_side_by_side_diff(&rows, width);
            assert!(!formatted.is_empty());
        }
    }
}
