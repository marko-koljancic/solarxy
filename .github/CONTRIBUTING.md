# Contributing to Solarxy

Contributions are welcome. How to build, what the conventions are, and how a change gets
reviewed all live on the
[Contributing wiki page](https://github.com/marko-koljancic/solarxy/wiki/Contributing).

This file covers the one thing that page cannot: **the terms a contribution arrives under.**
Solarxy is licensed under the GNU General Public License, version 3 or later; see
[LICENSE](../LICENSE).

## What you agree to by opening a pull request

Two things, and neither takes your copyright away from you.

### 1. Sign off on the origin of your work

Every commit must carry a `Signed-off-by` line, which `git commit -s` adds for you using your
configured name and email. That line means you certify the Developer Certificate of Origin
below. Its point is narrow and practical: it is a statement that the code is yours to give, so
that the project is not quietly absorbing someone else's work.

```
Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

### 2. Grant a license broad enough to keep the project's options open

**You keep the copyright in your contribution.** In addition to the GPL, you grant the project
maintainer a perpetual, worldwide, non-exclusive, royalty-free, irrevocable license to
reproduce, prepare derivative works of, publicly display, publicly perform, **sublicense** and
distribute your contribution and such derivative works, **including under license terms other
than the GNU General Public License.**

You also confirm that you are legally entitled to grant that license: the work is yours, or you
have permission from whoever owns it, and it is not encumbered by an employment or client
agreement that would contradict this.

## Why that second clause exists

Without the right to sublicense, the license the project ships under is frozen the moment the
first outside contribution is merged. Changing it afterwards would mean tracing every
contributor and getting each one's agreement, which in practice means it never happens.

The clause is not a plan to close the source. Solarxy is copyleft and the intent is that it
stays that way. What the clause preserves is the ability to answer questions that are not yet
answerable: whether a library crate other projects want to embed should ship under permissive
terms, whether a future release adopts a later GPL version, and whether a commercial exception
is ever offered alongside the free license. Those decisions become impossible, rather than
merely undecided, without it.

If you are not comfortable with the grant, say so in the pull request rather than quietly not
signing off. It is a conversation worth having, and a contribution can often be reshaped as an
issue, a reproduction case, or a review comment that carries no licensing question at all.

## Third-party code

Do not paste code from another project without saying so. If a change ports, adapts, or
vendors someone else's work:

- Its license must be compatible with GPL-3.0-or-later, or be covered by the additional
  permission recorded in [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md).
- Say in the pull request what was taken, from where, and under what terms.
- Add the attribution and license text to `THIRD-PARTY-NOTICES.md` in the same change. That
  file ships inside every distribution, which is what makes it the notice rather than a list.

An algorithm from a paper is not a licensing question, but it is still a citation. Those go in
the same file, in the section that credits published algorithms.
