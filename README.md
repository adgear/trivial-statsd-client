A Rust statsd client for fast-and-loose stats.

Alternatives:
 - https://github.com/tshlabs/cadence
 - https://github.com/markstory/rust-statsd
 - https://github.com/erik/rust-statsd

Why this one?
 - tuned for heavily-sampled stats;
 - avoids floats at runtime wherever possible;
 - uses evil global state for the convenience of being able to drop a
   `statsd!` macro anywhere (using the same approach as the log
   crate).

(This might not be true yet; this crate is in embryonic state and I
think I'll have to do some procedural macro magic to get the effect I
want.)
