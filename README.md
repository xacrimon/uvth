# uvth

Compact and efficient threadpool implementation as an alternative to the threadpool crate.

uvth is a more efficient alternative to threadpool with less overhead.

## benchmarks

```
i7-7700HQ 4C/8T Clear Linux

Time taken to run 100k no-op (empty) jobs. Averages from 5050 iterations.

threadpool_crate        time:   [42.201 ms 42.373 ms 42.560 ms]

threadpool_uvth         time:   [4.4844 ms 4.5123 ms 4.5412 ms]
```
