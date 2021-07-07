use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;

use tokio::time::Instant;

use common::io::Stream;
use std::error::Error;
use std::fmt::Debug;
use std::sync::Arc;
use tokio;

pub trait CacheKey: Eq + Hash + Debug {}
impl<T: Eq + Hash + Debug> CacheKey for T {}

struct CacheEntry {
    stream: Arc<tokio::sync::Mutex<Stream>>,
    last_activity: Instant,
}

pub(crate) struct StreamsCache<F: Fn() -> Result<Stream, Box<dyn Error>>, K: CacheKey> {
    new_stream_creator: F,
    entries: std::sync::Mutex<HashMap<K, CacheEntry>>,
}

impl<F: Fn() -> Result<Stream, Box<dyn Error>>, K: CacheKey> StreamsCache<F, K> {
    pub(crate) fn new(new_stream_creator: F) -> Self {
        Self {
            new_stream_creator,
            entries: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn get(&self, key: K) -> Result<Arc<tokio::sync::Mutex<Stream>>, Box<dyn Error>> {
        let new_stream_creator = &self.new_stream_creator;
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();
        log::debug!("got key {:?}, entries size is {}", key, entries.len());
        let entry = match entries.entry(key) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(CacheEntry {
                stream: Arc::new(tokio::sync::Mutex::new(new_stream_creator()?)),
                last_activity: now,
            }),
        };
        entry.last_activity = now;
        Ok(entry.stream.clone())
    }
}
