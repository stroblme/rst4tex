# RST4TEX

Some very opinionated tools to cleanup a LaTeX document written in Rust.

- `fixtex`: formats a given LaTeX document by havin a new line after each sentence
- `fixbib`: scans a given LaTeX document for bibliography files, removes duplicates, sorts references by date and unifies citation keys by `[auth:lower][veryshorttitle:lower][year]`

## Install

There is a makefile available to build and install these scripts.
Just run

```bash
make
```

and

```bash
make install
```

to have the compiled binaries available in `$HOME/.local/bin`.