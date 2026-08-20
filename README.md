# rst4tex

A collection of (opinionated) tools written in Rust to improve code quality of a LaTeX document.

- **fixbib**: scans a given LaTeX document and its `\include{}`, `\input{}` and `\subfile{}` files for bibliography files and
  - removes duplicates (also those that only become visible once the citation keys are unified)
  - removes unused references (unless `--no-delete` is given)
  - sorts references by date when using multi-citations
  - unifies citation keys to `[auth:lower][veryshorttitle:lower][year]`
  - updates bibliography files and rewrites document citations
- **fixtex**: formats a given LaTeX document to
  - all the basic formatting rules (double white- and trailing whitespaces, double return lines, etc.)
  - add a new line after each sentence (or removing a new line if a sentence continues)
  - add proper indentions to each environment (except `document` and `section`s)
  - split multi-sentence `\caption{}` (and `\subcaption{}`) bodies one sentence per line

Note: adding a new line after each sentence naturally can cause lines to be longer than the common 88 chars.

## Install

Requires `rustc`, `make`, `curl` and `tar`.

```bash
curl -fsSL https://raw.githubusercontent.com/stroblme/rst4tex/main/install.sh | sh
```

This builds `fixtex` and `fixbib` and installs them to `$HOME/.local/bin`
(override with `INSTALL_DIR=/somewhere/else`, pick a branch or tag with `REF=v1.0`).

Or clone the repository and run `make`, which does the same thing.

## Usage

These scripts are intended to be registered as git hooks, but of course you can run them manually:

**fixtex**
```bash
fixtex main.tex
```

**fixbib**
```bash
fixbib main.tex
fixbib main.tex --no-delete  # keep references that are not cited anywhere
```

Both tools read the LaTeX document from the given path and update files in place.
`.bak` files are created, but I strongly recommend you to have a version control.

> No backup, no mercy.

## Related Work

- Checkout [tex-fmt](https://github.com/wgunderwood/tex-fmt) for a more advanced and comprehensive LaTex formatter.
- Of course the standard [latexindent](https://github.com/cmhughes/latexindent.pl) is the go-to for a versatile and battle-proven LaTex formatter.

Nothing to complain about these great tools.
I just found that they don't match my needs.