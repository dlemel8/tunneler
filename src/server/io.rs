use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

use common::io::Stream;

pub trait CacheKey: Eq + Hash + Debug {}
impl<T: Eq + Hash + Debug> CacheKey for T {}

pub trait StreamCreator: Fn() -> Result<Stream, Box<dyn Error>> + Send + Sync + 'static {}
impl<T: Fn() -> Result<Stream, Box<dyn Error>> + Send + Sync + 'static> StreamCreator for T {}

struct CacheEntry {
    stream: Arc<AsyncMutex<Stream>>,
    last_activity: Instant,
}

pub(crate) struct StreamsCache<F: StreamCreator, K: CacheKey> {
    new_stream_creator: F,
    entries: Mutex<HashMap<K, CacheEntry>>,
}

impl<F: StreamCreator, K: CacheKey> StreamsCache<F, K> {
    pub(crate) fn new(new_stream_creator: F) -> Self {
        Self {
            new_stream_creator,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn get(&self, key: K) -> Result<Arc<AsyncMutex<Stream>>, Box<dyn Error>> {
        let new_stream_creator = &self.new_stream_creator;
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();
        log::debug!("got key {:?}, entries size is {}", key, entries.len());
        let entry = match entries.entry(key) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(CacheEntry {
                stream: Arc::new(AsyncMutex::new(new_stream_creator()?)),
                last_activity: now,
            }),
        };
        entry.last_activity = now;
        Ok(entry.stream.clone())
    }
}
