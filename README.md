trivial-statsd-client
----
A Rust statsd client for fast-and-loose metrics.

## Alternatives:
 - https://github.com/tshlabs/cadence
 - https://github.com/markstory/rust-statsd
 - https://github.com/erik/rust-statsd

## Why this one?
 - supports subsampling (rate)
 - avoids floats wherever possible
 
## Testing it
 
 Use `cargo test`, duh.
 
 For manual integrated testing with a real local statsd server:
 - Clone `https://github.com/etsy/statsd`
 - Copy `exmapleConfig.js` to `config.js` and add the `dumpMessages: true,` directive
 - Start the server `node stats.js config.js`
 - Run the cargo tests
 - See raw stats appear on statsd stdout 
 
## To improve:
 - Send multiple metrics per packet, either in explicit transaction or time+volume managed 
 - Make sampling apply to group of operations (channel open / commit)
 - Reuse packet-assembling string buffers **OR** 
 - Use `iovec` crate + friend for scatter/ gather instead of copying to an intermediary strbuf  
 - For lower sampling rates (< 1/256?), use predictive subsampling:
    only call rng after every accepted sample to approximate next sample distance, 
    then just inc a counter until we get there, sample, repeat
    rng would fuzz around target rate according to some factor  
    e.g. if rate is 1/2000, next sample would be between 1000 and 3000 points away
    counter needs to be per metric or per channel to make sure samples are evenly distributed across metrics 
 - Write more tests
