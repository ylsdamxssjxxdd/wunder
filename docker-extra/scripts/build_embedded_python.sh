#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PYTHON_VERSION="${PYTHON_VERSION:-3.11.5}"
PYTHON_PREFIX="${PYTHON_PREFIX:-/opt/python}"
BUILD_ROOT="${BUILD_ROOT:-${ROOT_DIR}/.build/python}"
SRC_DIR="${BUILD_ROOT}/src"
STAGE_DIR="${BUILD_ROOT}/stage"
PYTHON_ROOT="${STAGE_DIR}${PYTHON_PREFIX}"
REQ_FILE="${REQ_FILE:-${ROOT_DIR}/packaging/python/requirements-full.txt}"
REQ_FILE_EFFECTIVE="${REQ_FILE}"
WHEELHOUSE_DIR="${WHEELHOUSE_DIR:-${BUILD_ROOT}/wheelhouse}"
REPORTS_DIR="${REPORTS_DIR:-${BUILD_ROOT}/reports}"
# Allow source fallback for a small set of pure-python packages without wheels on arm64.
SOURCE_FALLBACK_PACKAGES="${SOURCE_FALLBACK_PACKAGES:-odfpy,cinrad,cartopy}"
REQUIRED_IMPORTS="${REQUIRED_IMPORTS:-matplotlib=matplotlib,cartopy=cartopy,pyproj=pyproj,shapely=shapely,netCDF4=netCDF4,cftime=cftime,h5py=h5py,cinrad=cinrad}"
REPAIR_MISSING_IMPORTS="${REPAIR_MISSING_IMPORTS:-1}"
EXTRA_REQUIREMENTS="${EXTRA_REQUIREMENTS:-}"
INCLUDE_PLAYWRIGHT="${INCLUDE_PLAYWRIGHT:-0}"
PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-${PYTHON_ROOT}/playwright}"
CARTOPY_DATA_DIR="${CARTOPY_DATA_DIR:-${PYTHON_ROOT}/share/cartopy}"
CARTOPY_DATA_LEVELS="${CARTOPY_DATA_LEVELS:-110m,50m,10m}"
CARTOPY_FEATURES="${CARTOPY_FEATURES:-coastline,land,ocean,lakes,rivers_lake_centerlines,admin_0_boundary_lines_land,admin_0_countries}"
CARTOPY_DOWNLOAD="${CARTOPY_DOWNLOAD:-1}"
ARM_PYART_BUILD="${ARM_PYART_BUILD:-auto}"
ARM_PYART_VERSION="${ARM_PYART_VERSION:-2.2.0}"
ARCH="${ARCH:-$(uname -m 2>/dev/null || true)}"
SETUPTOOLS_SPEC="${SETUPTOOLS_SPEC:-setuptools<81}"
BUILD_HELPER_REQUIREMENTS="${BUILD_HELPER_REQUIREMENTS:-setuptools_scm}"
CINRAD_BUILD="${CINRAD_BUILD:-auto}"

mkdir -p "${SRC_DIR}" "${STAGE_DIR}" "${WHEELHOUSE_DIR}" "${REPORTS_DIR}"

validate_python_imports() {
  local import_map=$1
  local report_path=${2:-}
  IMPORT_VALIDATION_MAP="${import_map}" \
  IMPORT_REPORT_PATH="${report_path}" \
    "${PYTHON_ROOT}/bin/python3" - <<'PY'
import importlib
import json
import os
import sys

mapping_raw = os.environ.get("IMPORT_VALIDATION_MAP", "")
report_path = os.environ.get("IMPORT_REPORT_PATH", "")
pairs = []
for item in mapping_raw.split(","):
    item = item.strip()
    if not item:
        continue
    if "=" in item:
        module_name, package_name = item.split("=", 1)
    else:
        module_name = item
        package_name = item
    module_name = module_name.strip()
    package_name = package_name.strip()
    if module_name and package_name:
        pairs.append((module_name, package_name))

results = []
missing_packages = []
for module_name, package_name in pairs:
    try:
        importlib.import_module(module_name)
        results.append({"module": module_name, "package": package_name, "ok": True})
    except Exception as exc:
        results.append(
            {
                "module": module_name,
                "package": package_name,
                "ok": False,
                "error": f"{type(exc).__name__}: {exc}",
            }
        )
        missing_packages.append(package_name)

if report_path:
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as fh:
        json.dump(results, fh, ensure_ascii=False, indent=2)

if missing_packages:
    ordered = list(dict.fromkeys(missing_packages))
    print("\n".join(ordered))
    raise SystemExit(1)
PY
}

repair_missing_python_packages() {
  local packages=("$@")
  if [ "${#packages[@]}" -eq 0 ]; then
    return 0
  fi
  echo "Repairing missing embedded Python packages: ${packages[*]}"
  "${PYTHON_ROOT}/bin/python3" -m pip download -d "${WHEELHOUSE_DIR}" --prefer-binary --no-build-isolation "${packages[@]}"
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation "${packages[@]}"
}

build_cinrad_from_source() {
  local cinrad_spec=${1:-cinrad}
  local cinrad_work="${BUILD_ROOT}/cinrad"
  rm -rf "${cinrad_work}"
  mkdir -p "${cinrad_work}"
  "${PYTHON_ROOT}/bin/python3" -m pip download --no-deps --no-binary=:all: --no-build-isolation "${cinrad_spec}" -d "${cinrad_work}"
  local cinrad_tar
  cinrad_tar="$(ls -1 "${cinrad_work}"/cinrad-*.tar.gz 2>/dev/null | head -n 1 || true)"
  if [ -z "${cinrad_tar}" ]; then
    echo "cinrad source tarball not found in ${cinrad_work}" >&2
    exit 1
  fi
  tar -xzf "${cinrad_tar}" -C "${cinrad_work}"
  local cinrad_src
  cinrad_src="$(find "${cinrad_work}" -maxdepth 1 -type d -name 'cinrad-*' | head -n 1 || true)"
  if [ -z "${cinrad_src}" ]; then
    echo "cinrad source directory not found after extract." >&2
    exit 1
  fi
  find "${cinrad_src}" -type f \( -name '_utils.c' -o -name '_unwrap_2d.c' \) -print0 | while IFS= read -r -d '' source_file; do
    sed -i 's|"longintrepr.h"|"cpython/longintrepr.h"|g' "${source_file}"
  done
  "${PYTHON_ROOT}/bin/python3" -m pip wheel --no-deps --no-build-isolation -w "${WHEELHOUSE_DIR}" "${cinrad_src}"
  local cinrad_wheel
  cinrad_wheel="$(ls -1t "${WHEELHOUSE_DIR}"/cinrad-*.whl 2>/dev/null | head -n 1 || true)"
  if [ -z "${cinrad_wheel}" ]; then
    echo "cinrad wheel not found in ${WHEELHOUSE_DIR}" >&2
    exit 1
  fi
  "${PYTHON_ROOT}/bin/python3" -m pip download -d "${WHEELHOUSE_DIR}" --find-links "${WHEELHOUSE_DIR}" --prefer-binary --no-build-isolation "${cinrad_spec}"
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation "${cinrad_spec}"
}

write_python_reports() {
  "${PYTHON_ROOT}/bin/python3" -m pip freeze > "${REPORTS_DIR}/stage-pip-freeze.txt"
  "${PYTHON_ROOT}/bin/python3" -m pip list --format json > "${REPORTS_DIR}/stage-pip-list.json"
}

if [ "${ARM_PYART_BUILD}" = "auto" ]; then
  if [ "${ARCH}" = "aarch64" ] || [ "${ARCH}" = "arm64" ]; then
    ARM_PYART_BUILD=1
  else
    ARM_PYART_BUILD=0
  fi
fi
if [ "${CINRAD_BUILD}" = "auto" ]; then
  if grep -q -E '^cinrad([<>=~]|$)' "${REQ_FILE}"; then
    CINRAD_BUILD=1
  else
    CINRAD_BUILD=0
  fi
fi
if [ "${ARM_PYART_BUILD}" = "1" ]; then
  REQ_FILE_EFFECTIVE="${BUILD_ROOT}/requirements.filtered.txt"
  grep -v -E '^arm_pyart([<>=~]|$)' "${REQ_FILE}" > "${REQ_FILE_EFFECTIVE}"
fi
if [ "${CINRAD_BUILD}" = "1" ]; then
  if [ "${REQ_FILE_EFFECTIVE}" = "${REQ_FILE}" ]; then
    REQ_FILE_EFFECTIVE="${BUILD_ROOT}/requirements.filtered.txt"
    grep -v -E '^cinrad([<>=~]|$)' "${REQ_FILE}" > "${REQ_FILE_EFFECTIVE}"
  else
    grep -v -E '^cinrad([<>=~]|$)' "${REQ_FILE_EFFECTIVE}" > "${REQ_FILE_EFFECTIVE}.tmp"
    mv "${REQ_FILE_EFFECTIVE}.tmp" "${REQ_FILE_EFFECTIVE}"
  fi
fi

if [ ! -x "${PYTHON_ROOT}/bin/python3" ]; then
  TARBALL="${SRC_DIR}/Python-${PYTHON_VERSION}.tgz"
  if [ ! -f "${TARBALL}" ]; then
    curl -fsSL "https://www.python.org/ftp/python/${PYTHON_VERSION}/Python-${PYTHON_VERSION}.tgz" -o "${TARBALL}"
  fi
  rm -rf "${SRC_DIR}/Python-${PYTHON_VERSION}"
  tar -xzf "${TARBALL}" -C "${SRC_DIR}"
  pushd "${SRC_DIR}/Python-${PYTHON_VERSION}" >/dev/null
  ./configure \
    --prefix="${PYTHON_PREFIX}" \
    --enable-shared \
    --with-ensurepip=install
  make -j"$(nproc)"
  make install DESTDIR="${STAGE_DIR}"
  popd >/dev/null
  if command -v patchelf >/dev/null 2>&1; then
    patchelf --set-rpath '$ORIGIN/../lib' "${PYTHON_ROOT}/bin/python3" || true
  fi
fi

export PYTHONHOME="${PYTHON_ROOT}"
export LD_LIBRARY_PATH="${PYTHON_ROOT}/lib:${LD_LIBRARY_PATH:-}"

"${PYTHON_ROOT}/bin/python3" -m pip install --upgrade pip "${SETUPTOOLS_SPEC}" wheel
"${PYTHON_ROOT}/bin/python3" -m pip download "${SETUPTOOLS_SPEC}" wheel -d "${WHEELHOUSE_DIR}" --only-binary=:all:
if [ -n "${BUILD_HELPER_REQUIREMENTS}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download ${BUILD_HELPER_REQUIREMENTS} -d "${WHEELHOUSE_DIR}" --only-binary=:all:
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation ${BUILD_HELPER_REQUIREMENTS}
fi
"${PYTHON_ROOT}/bin/python3" -m pip download numpy -d "${WHEELHOUSE_DIR}" --only-binary=:all:
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation numpy
"${PYTHON_ROOT}/bin/python3" -m pip download Cython -d "${WHEELHOUSE_DIR}" --only-binary=:all:
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation Cython
if [ -n "${SOURCE_FALLBACK_PACKAGES}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE_EFFECTIVE}" -d "${WHEELHOUSE_DIR}" \
    --only-binary=:all: \
    --no-binary "${SOURCE_FALLBACK_PACKAGES}" \
    --no-build-isolation
else
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE_EFFECTIVE}" -d "${WHEELHOUSE_DIR}" --only-binary=:all: --no-build-isolation
fi
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation -r "${REQ_FILE_EFFECTIVE}"

if [ "${INCLUDE_PLAYWRIGHT}" = "1" ]; then
  EXTRA_REQUIREMENTS="${EXTRA_REQUIREMENTS} playwright"
fi

if [ -n "${EXTRA_REQUIREMENTS}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download ${EXTRA_REQUIREMENTS} -d "${WHEELHOUSE_DIR}" --only-binary=:all:
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation ${EXTRA_REQUIREMENTS}
fi

if [ "${ARM_PYART_BUILD}" = "1" ]; then
  echo "Building arm_pyart from source for ${ARCH}..."
  pyart_work="${BUILD_ROOT}/arm_pyart"
  rm -rf "${pyart_work}"
  mkdir -p "${pyart_work}"
  "${PYTHON_ROOT}/bin/python3" -m pip download --no-deps --no-binary=:all: "arm_pyart==${ARM_PYART_VERSION}" -d "${pyart_work}"
  pyart_tar="$(ls -1 "${pyart_work}"/arm_pyart-*.tar.gz 2>/dev/null | head -n 1 || true)"
  if [ -z "${pyart_tar}" ]; then
    echo "arm_pyart source tarball not found in ${pyart_work}" >&2
    exit 1
  fi
  tar -xzf "${pyart_tar}" -C "${pyart_work}"
  pyart_src="$(find "${pyart_work}" -maxdepth 1 -type d -name 'arm_pyart-*' | head -n 1 || true)"
  if [ -z "${pyart_src}" ]; then
    echo "arm_pyart source directory not found after extract." >&2
    exit 1
  fi
  "${PYTHON_ROOT}/bin/python3" - "${pyart_src}" <<'PY'
import re
from pathlib import Path
import sys

src_dir = Path(sys.argv[1])
setup_path = src_dir / "setup.py"
text = setup_path.read_text(encoding="utf-8")
text = text.replace(".pyx", ".c")
text = re.sub(
    r"ext_modules=cythonize\\([\\s\\S]*?\\),",
    "ext_modules=extensions,",
    text,
    count=1,
)
setup_path.write_text(text, encoding="utf-8")
PY
  SETUPTOOLS_SCM_PRETEND_VERSION="${ARM_PYART_VERSION}" \
    "${PYTHON_ROOT}/bin/python3" -m pip wheel --no-deps --no-build-isolation -w "${WHEELHOUSE_DIR}" "${pyart_src}"
  "${PYTHON_ROOT}/bin/python3" -m pip download -d "${WHEELHOUSE_DIR}" --find-links "${WHEELHOUSE_DIR}" --prefer-binary --no-build-isolation "arm_pyart==${ARM_PYART_VERSION}"
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" "arm_pyart==${ARM_PYART_VERSION}"
fi

REQUIRED_IMPORTS_EFFECTIVE="${REQUIRED_IMPORTS}"
if grep -q -E '^arm_pyart([<>=~]|$)' "${REQ_FILE}"; then
  REQUIRED_IMPORTS_EFFECTIVE="${REQUIRED_IMPORTS_EFFECTIVE},pyart=arm_pyart"
fi

validation_output=""
if ! validation_output=$(validate_python_imports \
  "${REQUIRED_IMPORTS_EFFECTIVE}" \
  "${REPORTS_DIR}/stage-import-validation.json"); then
  mapfile -t missing_packages < <(printf '%s\n' "${validation_output}" | sed '/^[[:space:]]*$/d')
  if [ "${REPAIR_MISSING_IMPORTS}" = "1" ] && [ "${#missing_packages[@]}" -gt 0 ]; then
    repair_missing_python_packages "${missing_packages[@]}"
    validate_python_imports \
      "${REQUIRED_IMPORTS_EFFECTIVE}" \
      "${REPORTS_DIR}/stage-import-validation.json" >/dev/null
  else
    echo "Embedded Python import validation failed." >&2
    exit 1
  fi
fi

if [ "${CARTOPY_DOWNLOAD}" = "1" ]; then
  export CARTOPY_DATA_DIR
  export CARTOPY_DATA_LEVELS
  export CARTOPY_FEATURES
  "${PYTHON_ROOT}/bin/python3" - <<'PY'
import os
import sys

data_dir = os.environ.get("CARTOPY_DATA_DIR")
levels = os.environ.get("CARTOPY_DATA_LEVELS", "")
features = os.environ.get("CARTOPY_FEATURES", "")

if not data_dir:
    sys.exit(0)

os.makedirs(data_dir, exist_ok=True)

try:
    import cartopy
    from cartopy import config as cartopy_config
    from cartopy.io import shapereader
except Exception as exc:
    print(f"[cartopy] not available: {exc}", file=sys.stderr)
    sys.exit(1)

cartopy_config["data_dir"] = data_dir

levels_list = [x.strip() for x in levels.split(",") if x.strip()]
features_list = [x.strip() for x in features.split(",") if x.strip()]

if not levels_list or not features_list:
    sys.exit(0)

def category_for(name: str) -> str:
    if name.startswith("admin_") or name.endswith("_countries") or name.endswith("_states_provinces"):
        return "cultural"
    return "physical"

errors = []
for level in levels_list:
    for name in features_list:
        category = category_for(name)
        try:
            shapereader.natural_earth(resolution=level, category=category, name=name)
        except Exception as exc:
            errors.append(f"{level}/{category}/{name}: {exc}")

if errors:
    print("[cartopy] download failed:", file=sys.stderr)
    for item in errors:
        print(f"  - {item}", file=sys.stderr)
    sys.exit(1)
PY
fi

if [ "${INCLUDE_PLAYWRIGHT}" = "1" ]; then
  export PLAYWRIGHT_BROWSERS_PATH
  mkdir -p "${PLAYWRIGHT_BROWSERS_PATH}"
  for cache_dir in \
    "${BUILD_ROOT}/playwright-cache/ms-playwright" \
    "${BUILD_ROOT}/playwright-test/ms-playwright" \
    "${BUILD_ROOT}/ms-playwright"; do
    if [ -d "${cache_dir}" ]; then
      cp -a "${cache_dir}/." "${PLAYWRIGHT_BROWSERS_PATH}/"
      break
    fi
  done
  "${PYTHON_ROOT}/bin/python3" -m playwright install chromium
fi

PY_VER=$("${PYTHON_ROOT}/bin/python3" - <<'PY'
import sys
print(f"{sys.version_info.major}.{sys.version_info.minor}")
PY
)
echo "${PY_VER}" > "${PYTHON_ROOT}/.wunder-python-version"

find "${PYTHON_ROOT}" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "${PYTHON_ROOT}" -type f -name '*.pyc' -delete
write_python_reports
