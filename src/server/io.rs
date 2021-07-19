use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{Duration, Instant};

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
    idle_entry_timeout: Duration,
    cleanup_interval: Duration,
    last_cleanup: Instant,
}

impl<F: StreamCreator, K: CacheKey> StreamsCache<F, K> {
    pub(crate) fn new(
        new_stream_creator: F,
        idle_entry_timeout: Duration,
        cleanup_interval: Duration,
    ) -> Self {
        Self {
            new_stream_creator,
            entries: Mutex::new(HashMap::new()),
            idle_entry_timeout,
            cleanup_interval,
            last_cleanup: Instant::now(),
        }
    }

    pub(crate) fn get(
        &self,
        key: K,
        now: Instant,
    ) -> Result<Arc<AsyncMutex<Stream>>, Box<dyn Error>> {
        let res = self.get_or_create_stream(key, now);
        if now - self.last_cleanup > self.cleanup_interval {
            self.cleanup_old_idle_streams(now);
        };
        res
    }

    fn get_or_create_stream(
        &self,
        key: K,
        now: Instant,
    ) -> Result<Arc<AsyncMutex<Stream>>, Box<dyn Error>> {
        let new_stream_creator = &self.new_stream_creator;
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

    fn cleanup_old_idle_streams(&self, now: Instant) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, v| now - v.last_activity < self.idle_entry_timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;
    use tokio_test::io::Builder;

    #[test]
    fn stream_cache_get_new_stream_creator_failed() -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(
            || Err(String::from("bla").into()),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let res = cache.get("bla", Instant::now());
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn stream_cache_get_new_stream_success() -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );

        let now = Instant::now();
        cache.get("bla", now)?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("bla"));
        assert_eq!(entries.get("bla").unwrap().last_activity, now);
        Ok(())
    }

    #[test]
    fn stream_cache_get_new_stream_another_exists() -> Result<(), Box<dyn Error>> {
        let t1 = Instant::now();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        {
            let mut entries = cache.entries.lock().unwrap();
            let entry = CacheEntry {
                stream: Arc::new(AsyncMutex::new(Stream::new(
                    Builder::new().build(),
                    Builder::new().build(),
                ))),
                last_activity: t1,
            };
            entries.insert("bli", entry);
        }

        let mut t2 = t1.clone();
        t2 += Duration::from_secs(1);
        cache.get("bla", t2)?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains_key("bla"));
        assert_eq!(entries.get("bla").unwrap().last_activity, t2);
        assert!(entries.contains_key("bli"));
        assert_eq!(entries.get("bli").unwrap().last_activity, t1);
        Ok(())
    }

    #[test]
    fn stream_cache_get_existing_stream_success() -> Result<(), Box<dyn Error>> {
        let mut now = Instant::now();
        let cache = StreamsCache::new(
            || Err(String::from("bla").into()),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        {
            let mut entries = cache.entries.lock().unwrap();
            let entry = CacheEntry {
                stream: Arc::new(AsyncMutex::new(Stream::new(
                    Builder::new().build(),
                    Builder::new().build(),
                ))),
                last_activity: now,
            };
            entries.insert("bla", entry);
        }

        now += Duration::from_secs(1);
        cache.get("bla", now)?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("bla"));
        assert_eq!(entries.get("bla").unwrap().last_activity, now);
        Ok(())
    }

    #[test]
    fn stream_cache_get_auto_cleanup() -> Result<(), Box<dyn Error>> {
        let mut now = Instant::now();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(1),
            Duration::from_secs(1),
        );
        {
            let mut entries = cache.entries.lock().unwrap();
            let entry = CacheEntry {
                stream: Arc::new(AsyncMutex::new(Stream::new(
                    Builder::new().build(),
                    Builder::new().build(),
                ))),
                last_activity: now,
            };
            entries.insert("bli", entry);
        }

        now += Duration::from_secs(5);
        cache.get("bla", now)?;

        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("bla"));
        assert_eq!(entries.get("bla").unwrap().last_activity, now);
        Ok(())
    }
}
