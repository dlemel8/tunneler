use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;

use tokio::time::Instant;

use common::io::Stream;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

struct CacheEntry {
    stream: Arc<Mutex<Stream>>,
    last_activity: Instant,
}

pub(crate) struct StreamsCache<F: Fn() -> Result<Stream, Box<dyn Error>>, K: Eq + Hash> {
    new_stream_creator: F,
    entries: HashMap<K, CacheEntry>,
}

impl<F: Fn() -> Result<Stream, Box<dyn Error>>, K: Eq + Hash> StreamsCache<F, K> {
    pub(crate) fn new(new_stream_creator: F) -> Self {
        Self {
            new_stream_creator,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, key: K) -> Result<Arc<Mutex<Stream>>, Box<dyn Error>> {
        let new_stream_creator = &self.new_stream_creator;
        let now = Instant::now();
        let entry = match self.entries.entry(key) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(CacheEntry {
                stream: Arc::new(Mutex::new(new_stream_creator()?)),
                last_activity: now,
            }),
        };
        entry.last_activity = now;
        Ok(entry.stream.clone())
    }
}
