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

#[cfg(test)]
mod tests {
    use super::*;
    use common::io::{AsyncReadWrapper, AsyncWriteWrapper};
    use tokio_test::io::Builder;

    #[test]
    fn stream_cache_get_new_stream_creator_failed() -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(|| Err(String::from("bla").into()));
        let res = cache.get("bla");
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn stream_cache_get_new_stream_success() -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(|| {
            let untunneled_read_mock = Builder::new().build();
            let untunneled_reader = Box::new(AsyncReadWrapper::new(untunneled_read_mock));
            let untunneled_write_mock = Builder::new().build();
            let untunneled_writer = Box::new(AsyncWriteWrapper::new(untunneled_write_mock));
            Ok(Stream {
                reader: untunneled_reader,
                writer: untunneled_writer,
            })
        });

        cache.get("bla")?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("bla"));
        Ok(())
    }

    #[test]
    fn stream_cache_get_existing_stream_success() -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(|| Err(String::from("bla").into()));
        {
            let mut entries = cache.entries.lock().unwrap();
            let read_mock = Builder::new().build();
            let reader = Box::new(AsyncReadWrapper::new(read_mock));
            let write_mock = Builder::new().build();
            let writer = Box::new(AsyncWriteWrapper::new(write_mock));
            let entry = CacheEntry {
                stream: Arc::new(AsyncMutex::new(Stream { reader, writer })),
                last_activity: Instant::now(),
            };
            entries.insert("bla", entry);
        }

        cache.get("bla")?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("bla"));
        Ok(())
    }
}
