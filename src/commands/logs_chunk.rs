use alloy::rpc::types::{Filter, Log};
use eyre::Result;

pub struct ChunkPlanner {
    pub current_start: u64,
    pub to_block: u64,
    pub chunk_size: u64,
}

impl ChunkPlanner {
    pub fn new(from_block: u64, to_block: u64, initial_chunk: u64) -> Self {
        Self {
            current_start: from_block,
            to_block,
            chunk_size: initial_chunk,
        }
    }

    pub fn next_chunk(&self) -> Option<(u64, u64)> {
        if self.current_start > self.to_block {
            return None;
        }
        let end = std::cmp::min(
            self.current_start
                .saturating_add(self.chunk_size)
                .saturating_sub(1),
            self.to_block,
        );
        Some((self.current_start, end))
    }

    pub fn on_success(&mut self) {
        if let Some((_, end)) = self.next_chunk() {
            self.current_start = end.saturating_add(1);
        }
    }

    pub fn on_error(&mut self) -> bool {
        if self.chunk_size <= 1 {
            false
        } else {
            self.chunk_size = std::cmp::max(self.chunk_size / 2, 1);
            true
        }
    }
}

/// Fetch logs in chunks, halving chunk size on error (e.g., RPC range too large).
pub async fn fetch_logs_chunked(
    rpc_urls: &[String],
    filter: Filter,
    from_block: u64,
    to_block: u64,
    limit: usize,
) -> Result<Vec<Log>> {
    let mut logs = Vec::new();
    let mut planner = ChunkPlanner::new(from_block, to_block, 1000);

    while logs.len() < limit {
        let Some((start, end)) = planner.next_chunk() else {
            break;
        };

        let chunk_filter = filter.clone().from_block(start).to_block(end);

        match crate::rpc::get_logs_with_fallback(rpc_urls, chunk_filter).await {
            Ok(mut chunk_logs) => {
                logs.append(&mut chunk_logs);
                planner.on_success();
            }
            Err(err) => {
                if !planner.on_error() {
                    return Err(err);
                }
            }
        }
    }

    logs.truncate(limit);
    Ok(logs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_basic_chunks() {
        let mut p = ChunkPlanner::new(1, 2500, 1000);

        assert_eq!(p.next_chunk(), Some((1, 1000)));
        p.on_success();
        assert_eq!(p.next_chunk(), Some((1001, 2000)));
        p.on_success();
        assert_eq!(p.next_chunk(), Some((2001, 2500)));
        p.on_success();
        assert_eq!(p.next_chunk(), None);
    }

    #[test]
    fn planner_halves_on_error() {
        let mut p = ChunkPlanner::new(1, 2000, 1000);

        assert_eq!(p.next_chunk(), Some((1, 1000)));
        assert_eq!(p.on_error(), true);
        assert_eq!(p.next_chunk(), Some((1, 500)));
        p.on_success();

        assert_eq!(p.next_chunk(), Some((501, 1000))); // Chunk size remains 500
        p.on_success();
    }

    #[test]
    fn planner_returns_false_when_chunk_is_one() {
        let mut p = ChunkPlanner::new(1, 10, 1);

        assert_eq!(p.next_chunk(), Some((1, 1)));
        assert_eq!(p.on_error(), false); // Cannot halve 1
    }
}
