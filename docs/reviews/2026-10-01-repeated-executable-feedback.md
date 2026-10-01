# Repeated executable feedback — 2026-10-01

A fresh twelve-response Library classification workflow failed before writing code. The model repeatedly supplied the executable again as its first argument, producing calls such as `git git status` and `/usr/bin/python3 /usr/bin/python3`.

For an already executed nonzero, non-timeout command result, the host now adds an advisory hint when the first argument's filename matches the executable filename. The hint gives correct Git/Python examples and acknowledges that a repeated name may be intentional, such as a filename. Original arguments, stdout, stderr, exit status and timeout handling stay intact. No rejection, normalization or automatic retry is introduced.

Validation: 421 core tests passed, one ignored fixture; static lint, formatting and diff checks passed. Real-process tests prove exact argument preservation and one invocation per call, including an intentional repeated-name success. Tests also preserve ordinary failures, signal exits and timeouts without this hint. Hosted validation and a separate fresh Helheim classification workflow remain pending; the original blocked control is retained.
