# RST4TEX

Some very opinionated tools written in Rust to cleanup a LaTeX document.

- **fixtex**: formats a given LaTeX document to
  - add a new line after each sentence (or removing a new line if a sentence continues)
  - add proper indents to each environment
- **fixbib**: scans a given LaTeX document for bibliography files and
  - removes duplicates
  - sorts references by date
  - unifies citation keys to `[auth:lower][veryshorttitle:lower][year]`

## Install

There is a makefile available to build and install these scripts.
Just run

```bash
make all
```

to have the compiled binaries available in `$HOME/.local/bin`.


## Usage

The scripts are intended to be registered as git hooks, but of course you can run them manually:

**fixtex**
```bash
fixtex main.tex
```

**fixbib**
```bash
fixbib main.tex
```

You can append `--dry-run` to avoid writing changes to the files.
Generally, `.bak` files are created but I strongly recommend you to have a version control.
**No backup, no mercy.**