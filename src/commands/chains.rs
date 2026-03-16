// List/search chains from chainlist.org directory
// Exports `run` which prints matching chains with RPC counts

use eyre::Result;

pub fn run(query: Option<&str>) -> Result<()> {
    let entries = crate::chainlist::load()?;
    let results: Vec<&crate::chainlist::ChainEntry> = match query {
        Some(q) => crate::chainlist::search(&entries, q),
        None => {
            // Without query, show top chains by RPC count (most popular)
            let mut sorted: Vec<&crate::chainlist::ChainEntry> = entries.iter().collect();
            sorted.sort_by(|a, b| {
                crate::chainlist::filtered_rpcs(b)
                    .len()
                    .cmp(&crate::chainlist::filtered_rpcs(a).len())
            });
            sorted.truncate(30);
            sorted
        }
    };
    if results.is_empty() {
        println!("No chains found.");
        return Ok(());
    }
    println!(
        "{:<8} {:<30} {:<8} {:<6}",
        "ID", "Name", "Symbol", "RPCs"
    );
    println!("{}", "-".repeat(56));
    for entry in &results {
        let rpc_count = crate::chainlist::filtered_rpcs(entry).len();
        let name = if entry.name.len() > 28 {
            format!("{}...", &entry.name[..25])
        } else {
            entry.name.clone()
        };
        println!(
            "{:<8} {:<30} {:<8} {:<6}",
            entry.chain_id, name, entry.chain, rpc_count
        );
    }
    if query.is_none() {
        println!("\nShowing top 30 by RPC count. Use `crpc chains <query>` to search.");
    }
    Ok(())
}
