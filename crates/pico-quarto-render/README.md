# pico-quarto-render

Experimental batch renderer for QMD files to HTML.

This crate exists for prototyping and experimentation with Quarto's rendering pipeline. It is not intended for production use.

## Usage

```bash
pico-quarto-render <INPUT_DIR> <OUTPUT_DIR> [-v]
```

## Parallelism

Files are processed in parallel using [Rayon](https://docs.rs/rayon). To control the number of threads, set the `RAYON_NUM_THREADS` environment variable:

```bash
# Use 4 threads
RAYON_NUM_THREADS=4 pico-quarto-render input/ output/

# Use single thread (sequential processing, no rayon overhead)
RAYON_NUM_THREADS=1 pico-quarto-render input/ output/
```

If not set, Rayon defaults to the number of logical CPUs.

When `RAYON_NUM_THREADS=1`, the code bypasses Rayon entirely and uses a simple sequential loop. This produces cleaner stack traces for profiling.
