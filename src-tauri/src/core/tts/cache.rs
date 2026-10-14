//! LRU cache for synthesized WAV audio.

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioCacheKey {
    pub text: String,
    pub voice: String,
    pub speed_cent: u32, // (speed * 100).round() as u32
}

impl AudioCacheKey {
    pub fn new(text: impl Into<String>, voice: impl Into<String>, speed: f32) -> Self {
        Self {
            text: text.into(),
            voice: voice.into(),
            speed_cent: (speed.clamp(0.1, 5.0) * 100.0).round() as u32,
        }
    }
}

pub struct AudioLruCache {
    inner: Mutex<LruCache<AudioCacheKey, Vec<u8>>>,
}

impl AudioLruCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            inner: Mutex::new(LruCache::new(cap)),
        }
    }

    pub fn get(&self, key: &AudioCacheKey) -> Option<Vec<u8>> {
        self.inner.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: AudioCacheKey, wav_bytes: Vec<u8>) {
        if let Ok(mut lock) = self.inner.lock() {
            lock.put(key, wav_bytes);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut lock) = self.inner.lock() {
            lock.clear();
        }
    }
}

impl Default for AudioLruCache {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_get_clear() {
        let cache = AudioLruCache::new(2);
        let k1 = AudioCacheKey::new("hello", "af_heart", 1.0);
        let k2 = AudioCacheKey::new("world", "af_heart", 1.0);
        let k3 = AudioCacheKey::new("hello", "af_bella", 1.0);

        cache.put(k1.clone(), vec![1, 2, 3]);
        assert_eq!(cache.get(&k1), Some(vec![1, 2, 3]));

        // Different voice produces different key
        assert_eq!(cache.get(&k3), None);

        cache.put(k2.clone(), vec![4, 5, 6]);
        cache.clear();
        assert_eq!(cache.get(&k1), None);
        assert_eq!(cache.get(&k2), None);
    }
}
