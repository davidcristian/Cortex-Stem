//! The writer thread (`stem-icon-cache`): it does the queued jobs one at a
//! time and keeps the index of the entries on disk.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Instant, SystemTime};

use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use super::format::{encode, temp_name};
use super::index::{Index, Slot};
use super::stats::Counters;
use super::{CacheLimits, Job};

pub(super) fn writer_loop(
    dir: &Path,
    limits: CacheLimits,
    producer: &str,
    rx: &Receiver<Job>,
    counters: &Counters,
    drain_until: &Mutex<Option<Instant>>,
) {
    let mut index = Index::scan(dir);
    let evicted = index.evict(dir, limits, None);
    counters.evictions.fetch_add(evicted, Ordering::Relaxed);
    publish(&index, counters);
    let mut temp_seq: u64 = 0;

    for job in rx {
        // Past shutdown's drain budget, file work is dropped, not done.
        let out_of_time = drain_until
            .lock()
            .is_some_and(|until| Instant::now() >= until);
        match job {
            Job::Write { .. } if out_of_time => Counters::bump(&counters.abandoned),
            Job::Touch { .. } | Job::Remove { .. } if out_of_time => {}
            Job::Write {
                name,
                key,
                size,
                stamp,
                png,
            } => {
                let started = Instant::now();
                let bytes = encode(producer, &key.0, size, stamp, &png);
                temp_seq += 1;
                let temp = temp_name(&name, temp_seq);
                match write_atomic(dir, &name, &temp, &bytes) {
                    Ok(()) => {
                        Counters::bump(&counters.writes);
                        let now = SystemTime::now();
                        index.insert(
                            name.clone(),
                            Slot {
                                len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                                used: now,
                                persisted: now,
                            },
                        );
                        let evicted = index.evict(dir, limits, Some(&name));
                        counters.evictions.fetch_add(evicted, Ordering::Relaxed);
                    }
                    Err(e) => {
                        Counters::bump(&counters.write_failures);
                        log::debug!("icon cache: {} ({size} px) not written: {e}", key.0);
                    }
                }
                let took = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                counters.write_nanos.fetch_add(took, Ordering::Relaxed);
            }
            Job::Touch { name } => index.touch(dir, &name),
            Job::Remove { name } => {
                match fs::remove_file(dir.join(&name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => log::debug!("icon cache: corrupt {name} not deleted: {e}"),
                }
                index.forget(&name);
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
            Job::Shutdown => break,
            #[cfg(test)]
            Job::Stall(pause) => std::thread::sleep(pause),
        }
        publish(&index, counters);
    }
    log::debug!("stem-icon-cache writer stopped");
}

fn publish(index: &Index, counters: &Counters) {
    let entries = u64::try_from(index.slots.len()).unwrap_or(u64::MAX);
    counters.entries.store(entries, Ordering::Relaxed);
    counters.bytes.store(index.bytes, Ordering::Relaxed);
}

/// Write `bytes` to `dir/temp`, then rename it over `dir/name`. The
/// directory is recreated once if it went away while Stem ran.
fn write_atomic(dir: &Path, name: &str, temp: &str, bytes: &[u8]) -> io::Result<()> {
    match write_then_rename(dir, name, temp, bytes) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir)?;
            write_then_rename(dir, name, temp, bytes)
        }
        other => other,
    }
}

fn write_then_rename(dir: &Path, name: &str, temp: &str, bytes: &[u8]) -> io::Result<()> {
    let temp_path = dir.join(temp);
    let result = File::create(&temp_path)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temp_path, dir.join(name)));
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}
