// Compare legacy vs DP pairing over a corpus; print the first N differing
// rows with detail.
use diffuse::cluster::pairing::set_dp_engine;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Fixture {
    diff: String,
    contents: HashMap<String, String>,
}

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mirage-2025-08".into());
    let only: Option<String> = std::env::args().nth(2);
    let raw = std::fs::read_to_string(format!("tests/golden/{name}.json")).unwrap();
    let fx: Fixture = serde_json::from_str(&raw).unwrap();
    let legacy = diffuse::bin_pipeline::run_pipeline(&fx.diff, &fx.contents);
    set_dp_engine(true);
    let dp = diffuse::bin_pipeline::run_pipeline(&fx.diff, &fx.contents);
    set_dp_engine(false);

    let mut files = 0;
    'files: for (file_i, (lf, df)) in legacy.files.iter().zip(dp.files.iter()).enumerate() {
        if lf != df {
            files += 1;
            if let Some(o) = &only
                && lf.path != *o
            {
                continue;
            }
            println!("== {} ==", lf.path);
            println!(
                "  hunks: legacy {} vs dp {}",
                lf.hunks.len(),
                df.hunks.len()
            );
            for (hunk_i, (lh, dh)) in lf.hunks.iter().zip(df.hunks.iter()).enumerate() {
                if lh != dh {
                    println!(
                        " hunk {hunk_i}: row count {} vs {}",
                        lh.rows.len(),
                        dh.rows.len()
                    );
                    println!("  LEGACY ROWS:");
                    for (ri, r) in lh.rows.iter().enumerate() {
                        println!(
                            "    [{ri}] {:?} ol={:?} nl={:?} text={:?}",
                            r.kind, r.old_line, r.new_line, r.text
                        );
                    }
                    println!("  DP ROWS:");
                    for (ri, r) in dh.rows.iter().enumerate() {
                        println!(
                            "    [{ri}] {:?} ol={:?} nl={:?} text={:?}",
                            r.kind, r.old_line, r.new_line, r.text
                        );
                    }
                    let parse_files = diffuse::parse::parse_diff(&fx.diff);
                    if let Some(pf) = parse_files.get(file_i)
                        && let Some(ph) = pf.hunks.get(hunk_i)
                    {
                        println!("  CLUSTERS FOR HUNK {hunk_i}:");
                        for (si, seg) in diffuse::cluster::partition_hunk_segments(ph, false)
                            .iter()
                            .enumerate()
                        {
                            match seg {
                                diffuse::cluster::HunkSegment::Context(ctx) => {
                                    println!("    [Seg {si} Ctx] {:?}", ctx.text());
                                }
                                diffuse::cluster::HunkSegment::Cluster(c) => {
                                    println!(
                                        "    [Seg {si} Cluster] old_len={} new_len={}",
                                        c.old_rows.len(),
                                        c.new_rows.len()
                                    );
                                    for (oi, r) in c.old_rows.iter().enumerate() {
                                        println!(
                                            "      old[{oi}] ol={:?} {:?}",
                                            r.old_line(),
                                            r.text()
                                        );
                                    }
                                    for (ni, r) in c.new_rows.iter().enumerate() {
                                        println!(
                                            "      new[{ni}] nl={:?} {:?}",
                                            r.new_line(),
                                            r.text()
                                        );
                                    }
                                    let sol = diffuse::cluster::dp::solve_cluster(c, false);
                                    println!("      SOL: {:?}", sol.transitions);
                                }
                            }
                        }
                    }
                    continue 'files;
                }
                for (row_i, (lr, dr)) in lh.rows.iter().zip(dh.rows.iter()).enumerate() {
                    if lr != dr {
                        println!(" hunk {hunk_i} row {row_i}:");
                        println!(
                            "  legacy: {:?} nl={:?} ol={:?} text={:?} del_text={:?}",
                            lr.kind, lr.new_line, lr.old_line, lr.text, lr.del_text
                        );
                        if lr.spans != dr.spans {
                            println!("    legacy spans={:?}", lr.spans);
                            println!("    dp     spans={:?}", dr.spans);
                        }
                        println!(
                            "  dp:     {:?} nl={:?} ol={:?} text={:?} del_text={:?}",
                            dr.kind, dr.new_line, dr.old_line, dr.text, dr.del_text
                        );
                        continue 'files;
                    }
                }
            }
        }
    }
    println!("files differing: {files}");
}
