# Contributing to hwlens

hwlens is a small, independently developed project. Issues and pull
requests are welcome.

## Building

hwlens needs a C++23 compiler (GCC 14+ or Clang 19+) and CMake 3.25+.

```sh
CXX=g++-14 CC=gcc-14 cmake -B build -DCMAKE_BUILD_TYPE=Release -DHWLENS_GPU=ON
cmake --build build -j"$(nproc)"
ctest --test-dir build
```

## Pull requests

- One topic per pull request, with a short description of what changed
  and how it was tested.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `refactor:` ...).
- Keep the existing style: tabs for indentation, `//?` for explanatory
  comments, `//*` for declarations.
- Code must build without new warnings and `ctest` must pass.
- New sensors or data sources should degrade gracefully: if a file,
  library symbol or permission is missing, hide the value instead of
  failing.

## License

By contributing you agree that your contributions are licensed under the
Apache License 2.0 (see LICENSE and NOTICE).
