Wunder Desktop Windows 7 Runtime Note

This Win7 setup package no longer bundles Python or Git.

If you need Python/Git features such as:
- Python-based tools
- local scripts
- git clone / git status / patch workflows

please extract the matching `wunder补充包-win7-*.zip` into the install directory root.

After extraction, the install directory should contain:
- opt\python
- opt\git

Then restart Wunder Desktop. The Electron runtime will automatically prepend
the supplement package paths to PATH and prefer the extracted Python/Git.
