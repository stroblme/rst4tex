# rst4tex

A collection of very opinionated tools written in Rust to improve code quality of a LaTeX document.

- **fixtex**: formats a given LaTeX document to
  - all the basic formatting rules (double white- and trailing whitespaces, double return lines, etc.)
  - add a new line after each sentence (or removing a new line if a sentence continues)
  - add proper indentions to each environment (except `document` and `section`s)
- **fixbib**: scans a given LaTeX document for bibliography files and
  - removes duplicates
  - sorts references by date when using multi-citations
  - unifies citation keys to `[auth:lower][veryshorttitle:lower][year]`
  - updates bibliography files and rewrites document citations

## Install

No binaries are available yet, so you need to clone this repository first.
There is a makefile available to build and install these scripts.
Just run

```bash
make
```

to have the compiled binaries available in `$HOME/.local/bin`.

## Usage

These scripts are intended to be registered as git hooks, but of course you can run them manually:

**fixtex**
```bash
fixtex main.tex
```

**fixbib**
```bash
fixbib main.tex
```

Both tools read the LaTeX document from the given path and update files in place.
`.bak` files are created, but I strongly recommend you to have a version control.

> No backup, no mercy.

## Related Work

Checkout [tex-fmt](https://github.com/wgunderwood/tex-fmt) for a more advanced and comprehensive LaTex formatter.

Of course the standard [latexindent](https://github.com/cmhughes/latexindent.pl) is the go-to for a versatile and battle-proven LaTex formatter.

Nothing to complain about these tools.
I just found that they don't match my needs.