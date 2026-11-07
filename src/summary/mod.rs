//! Rendering of the `--show-info` datacenter / cluster summary.

use clap::ValueEnum;
use comfy_table::{
    modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS},
    presets::UTF8_FULL,
    Table,
};
use itertools::Itertools;
use serde::Serialize;

use crate::models::rvtools::Datacenter;

/// Output format for the `--show-info` summary.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum SummaryFormat {
    /// Box-drawn terminal table (default)
    #[default]
    Table,
    /// JSON object with `rows` and `totals`
    Json,
    /// CSV with a header row
    Csv,
    /// GitHub-flavoured Markdown table with a totals row
    Md,
}

use std::path::Path;

/// Infer an output format from a file path extension.
/// `.csv` → Csv, `.json` → Json, `.md`/`.markdown` → Md, anything else → Table.
pub fn infer_format(path: &Path) -> SummaryFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("csv") => SummaryFormat::Csv,
        Some("json") => SummaryFormat::Json,
        Some("md" | "markdown") => SummaryFormat::Md,
        _ => SummaryFormat::Table,
    }
}

/// Explicit `--format` wins; otherwise infer from `path` when writing a file; else Table.
pub fn resolve_format(explicit: Option<SummaryFormat>, path: Option<&Path>) -> SummaryFormat {
    if let Some(f) = explicit {
        return f;
    }
    path.map(infer_format).unwrap_or(SummaryFormat::Table)
}

#[derive(Serialize, Debug, PartialEq)]
struct Row<'a> {
    datacenter: &'a str,
    cluster: &'a str,
    capacity_tb: f64,
    vm_count: usize,
    image_level_tb: f64,
    agent_in_estimate_tb: f64,
    excluded_non_vadp_tb: f64,
}

#[derive(Serialize, Debug, PartialEq)]
struct Totals {
    capacity_tb: f64,
    vm_count: usize,
    image_level_tb: f64,
    agent_in_estimate_tb: f64,
    excluded_non_vadp_tb: f64,
}

#[derive(Serialize)]
struct JsonSummary<'a> {
    rows: Vec<Row<'a>>,
    totals: Totals,
}

/// Datacenters sorted by capacity, largest first.
fn sorted(dcs: &[Datacenter]) -> Vec<&Datacenter> {
    dcs.iter()
        .sorted_by(|a, b| b.capacity.total_cmp(&a.capacity))
        .collect()
}

fn totals(dcs: &[Datacenter]) -> Totals {
    let capacity_tb = dcs.iter().fold(0.0, |acc, x| acc + x.capacity);
    let agent_in_estimate_tb = dcs.iter().fold(0.0, |acc, x| acc + x.agent);
    Totals {
        capacity_tb,
        vm_count: dcs.iter().map(|x| x.vm_count).sum(),
        image_level_tb: capacity_tb - agent_in_estimate_tb,
        agent_in_estimate_tb,
        excluded_non_vadp_tb: dcs.iter().fold(0.0, |acc, x| acc + x.excluded),
    }
}

pub fn render(dcs: &[Datacenter], format: SummaryFormat) -> anyhow::Result<String> {
    Ok(match format {
        SummaryFormat::Table => render_table(dcs),
        SummaryFormat::Json => render_json(dcs)?,
        SummaryFormat::Csv => render_csv(dcs)?,
        SummaryFormat::Md => render_md(dcs),
    })
}

pub fn render_table(dcs: &[Datacenter]) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_header(vec![
            "Datacenter",
            "Cluster",
            "Capacity (TB)",
            "VM Count",
            "Image-level (TB)",
            "Agent, in estimate (TB)",
            "Not in estimate, non-VADP (TB)",
        ]);
    for x in sorted(dcs) {
        table.add_row(vec![
            x.name.to_string(),
            x.cluster.to_string(),
            format!("{:.2}", x.capacity),
            x.vm_count.to_string(),
            format!("{:.2}", x.capacity - x.agent),
            format!("{:.2}", x.agent),
            format!("{:.2}", x.excluded),
        ]);
    }
    format!("{table}\n")
}

pub fn render_json(dcs: &[Datacenter]) -> anyhow::Result<String> {
    let summary = JsonSummary {
        rows: sorted(dcs)
            .into_iter()
            .map(|x| Row {
                datacenter: &x.name,
                cluster: &x.cluster,
                capacity_tb: x.capacity,
                vm_count: x.vm_count,
                image_level_tb: x.capacity - x.agent,
                agent_in_estimate_tb: x.agent,
                excluded_non_vadp_tb: x.excluded,
            })
            .collect(),
        totals: totals(dcs),
    };
    Ok(serde_json::to_string_pretty(&summary)? + "\n")
}

pub fn render_csv(dcs: &[Datacenter]) -> anyhow::Result<String> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "datacenter",
        "cluster",
        "capacity_tb",
        "vm_count",
        "image_level_tb",
        "agent_in_estimate_tb",
        "excluded_non_vadp_tb",
    ])?;
    for x in sorted(dcs) {
        wtr.write_record([
            x.name.as_str(),
            x.cluster.as_str(),
            &format!("{:.2}", x.capacity),
            &x.vm_count.to_string(),
            &format!("{:.2}", x.capacity - x.agent),
            &format!("{:.2}", x.agent),
            &format!("{:.2}", x.excluded),
        ])?;
    }
    Ok(String::from_utf8(wtr.into_inner()?)?)
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|")
}

pub fn render_md(dcs: &[Datacenter]) -> String {
    let mut out =
        String::from("| Datacenter | Cluster | Capacity (TB) | VM Count | Image-level (TB) | Agent, in estimate (TB) | Not in estimate, non-VADP (TB) |\n|---|---|---:|---:|---:|---:|---:|\n");
    for x in sorted(dcs) {
        out.push_str(&format!(
            "| {} | {} | {:.2} | {} | {:.2} | {:.2} | {:.2} |\n",
            md_escape(&x.name),
            md_escape(&x.cluster),
            x.capacity,
            x.vm_count,
            x.capacity - x.agent,
            x.agent,
            x.excluded
        ));
    }
    let t = totals(dcs);
    out.push_str(&format!(
        "| **Total** | | **{:.2}** | **{}** | **{:.2}** | **{:.2}** | **{:.2}** |\n",
        t.capacity_tb, t.vm_count, t.image_level_tb, t.agent_in_estimate_tb, t.excluded_non_vadp_tb
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc(name: &str, cluster: &str, vm_count: usize, capacity: f64) -> Datacenter {
        Datacenter {
            name: name.into(),
            cluster: cluster.into(),
            vm_count,
            capacity,
            ..Default::default()
        }
    }

    fn sample() -> Vec<Datacenter> {
        vec![
            dc("DC-A", "C1", 3, 1.5),
            dc("DC-B", "C2", 5, 10.256),
            dc("DC,C", "C|3", 1, 0.004),
        ]
    }

    #[test]
    fn csv_sorted_escaped_two_decimals() {
        let out = render_csv(&sample()).unwrap();
        assert_eq!(
            out,
            "datacenter,cluster,capacity_tb,vm_count,image_level_tb,agent_in_estimate_tb,excluded_non_vadp_tb\nDC-B,C2,10.26,5,10.26,0.00,0.00\nDC-A,C1,1.50,3,1.50,0.00,0.00\n\"DC,C\",C|3,0.00,1,0.00,0.00,0.00\n"
        );
    }

    #[test]
    fn md_has_totals_and_escapes_pipes() {
        let out = render_md(&sample());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0],
            "| Datacenter | Cluster | Capacity (TB) | VM Count | Image-level (TB) | Agent, in estimate (TB) | Not in estimate, non-VADP (TB) |"
        );
        assert_eq!(lines[2], "| DC-B | C2 | 10.26 | 5 | 10.26 | 0.00 | 0.00 |");
        assert_eq!(lines[4], "| DC,C | C\\|3 | 0.00 | 1 | 0.00 | 0.00 | 0.00 |");
        assert_eq!(
            lines[5],
            "| **Total** | | **11.76** | **9** | **11.76** | **0.00** | **0.00** |"
        );
    }

    #[test]
    fn json_rows_and_totals() {
        let v: serde_json::Value = serde_json::from_str(&render_json(&sample()).unwrap()).unwrap();
        assert_eq!(v["rows"][0]["datacenter"], "DC-B");
        assert_eq!(v["rows"][0]["capacity_tb"], 10.256);
        assert_eq!(v["rows"][2]["cluster"], "C|3");
        assert_eq!(v["totals"]["vm_count"], 9);
        assert_eq!(v["rows"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn table_contains_rows_in_order() {
        let out = render_table(&sample());
        assert!(out.contains("Capacity (TB)"));
        assert!(out.find("DC-B").unwrap() < out.find("DC-A").unwrap());
        assert!(out.contains("10.26"));
    }

    #[test]
    fn infer_format_from_extension() {
        use std::path::Path;
        assert_eq!(infer_format(Path::new("out.csv")), SummaryFormat::Csv);
        assert_eq!(infer_format(Path::new("OUT.JSON")), SummaryFormat::Json);
        assert_eq!(infer_format(Path::new("a.md")), SummaryFormat::Md);
        assert_eq!(infer_format(Path::new("a.markdown")), SummaryFormat::Md);
        assert_eq!(infer_format(Path::new("a.txt")), SummaryFormat::Table);
        assert_eq!(infer_format(Path::new("noext")), SummaryFormat::Table);
    }

    #[test]
    fn resolve_format_explicit_wins() {
        use std::path::Path;
        assert_eq!(
            resolve_format(Some(SummaryFormat::Json), Some(Path::new("x.csv"))),
            SummaryFormat::Json
        );
        assert_eq!(
            resolve_format(None, Some(Path::new("x.csv"))),
            SummaryFormat::Csv
        );
        assert_eq!(resolve_format(None, None), SummaryFormat::Table);
    }

    #[test]
    fn empty_input() {
        assert_eq!(
            render_csv(&[]).unwrap(),
            "datacenter,cluster,capacity_tb,vm_count,image_level_tb,agent_in_estimate_tb,excluded_non_vadp_tb\n"
        );
        assert!(render_md(&[]).contains("**0.00**"));
        let v: serde_json::Value = serde_json::from_str(&render_json(&[]).unwrap()).unwrap();
        assert_eq!(v["rows"].as_array().unwrap().len(), 0);
    }
}
