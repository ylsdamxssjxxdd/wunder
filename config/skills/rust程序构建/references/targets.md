# Target and release gates

- `win7-x86`: PE32/i386 (`0x14c`), Win7 compatible imports, MinGW runtime present. Do not use APIs introduced after Windows 7.
- `linux-amd64-ubuntu18`: ELF x86-64, glibc symbols no newer than 2.27.
- `linux-arm64-ubuntu18`: ELF AArch64, glibc symbols no newer than 2.27.
- Kylin x86 使用与 `linux-amd64-ubuntu18` 相同的 x86-64 ELF 门禁；不要再区分独立 ABI。

Release output should be versioned, stripped when safe, accompanied by SHA-256, target triple, toolchain and SDK manifest hash. Test on the target OS when possible.
