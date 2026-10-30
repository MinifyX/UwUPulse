# What the plugin builds against, as buildspec.json pins it (like the
# official obs-plugintemplate): libobs and obs-frontend-api built from the
# OBS sources against the prebuilt obs-deps, Qt 6 from obs-deps, and
# obs-browser's panel header. Every download is checked against its SHA-256.
#
#   cmake -DPLATFORM=macos|windows-x64|windows-arm64|linux -DDEPS_DIR=<dir> -P deps.cmake
#
# Linux takes libobs, obs-frontend-api and Qt from the system packages
# (obs-studio from the OBS PPA, qt6-base-dev), so it only fetches the header.
#
# Writes <DEPS_DIR>/prefix-path.txt: the CMAKE_PREFIX_PATH for the plugin.
# A second run with the same buildspec does nothing.

cmake_minimum_required(VERSION 3.28)

if(NOT PLATFORM OR NOT DEPS_DIR)
  message(FATAL_ERROR "usage: cmake -DPLATFORM=<platform> -DDEPS_DIR=<dir> -P deps.cmake")
endif()
if(NOT PLATFORM MATCHES "^(macos|windows-x64|windows-arm64|linux)$")
  message(FATAL_ERROR "unknown PLATFORM ${PLATFORM}")
endif()

get_filename_component(DEPS_DIR "${DEPS_DIR}" ABSOLUTE)
set(spec_file "${CMAKE_CURRENT_LIST_DIR}/../buildspec.json")
file(READ "${spec_file}" spec)
file(SHA256 "${spec_file}" spec_hash)
set(stamp "${DEPS_DIR}/.stamp-${PLATFORM}")

if(EXISTS "${stamp}")
  file(READ "${stamp}" old_hash)
  if(old_hash STREQUAL spec_hash)
    message(STATUS "OBS dependencies for ${PLATFORM} are up to date in ${DEPS_DIR}")
    return()
  endif()
endif()

file(MAKE_DIRECTORY "${DEPS_DIR}/downloads")

# download(<url> <file> <sha256>): fetches unless there already, checks the
# hash either way.
function(download url file sha)
  set(path "${DEPS_DIR}/downloads/${file}")
  if(EXISTS "${path}")
    file(SHA256 "${path}" have)
    if(NOT have STREQUAL sha)
      file(REMOVE "${path}")
    endif()
  endif()
  if(NOT EXISTS "${path}")
    message(STATUS "Downloading ${url}")
    file(DOWNLOAD "${url}" "${path}" EXPECTED_HASH SHA256=${sha} STATUS st TLS_VERIFY ON)
    list(GET st 0 code)
    if(NOT code EQUAL 0)
      file(REMOVE "${path}")
      message(FATAL_ERROR "download failed: ${url}: ${st}")
    endif()
  endif()
endfunction()

# extract(<file> <dir>): a fresh <dir> with the archive's contents.
function(extract file dir)
  file(REMOVE_RECURSE "${dir}")
  file(MAKE_DIRECTORY "${dir}")
  file(ARCHIVE_EXTRACT INPUT "${DEPS_DIR}/downloads/${file}" DESTINATION "${dir}")
endfunction()

# obs-browser's panel header (QCef, QCefWidget).
string(JSON url GET "${spec}" obs-browser url)
string(JSON sha GET "${spec}" obs-browser sha256)
download("${url}" browser-panel.hpp ${sha})
file(MAKE_DIRECTORY "${DEPS_DIR}/obs-browser/panel")
file(COPY_FILE "${DEPS_DIR}/downloads/browser-panel.hpp" "${DEPS_DIR}/obs-browser/panel/browser-panel.hpp")

if(PLATFORM STREQUAL "linux")
  file(WRITE "${DEPS_DIR}/prefix-path.txt" "")
  file(WRITE "${stamp}" "${spec_hash}")
  return()
endif()

foreach(dep prebuilt qt6)
  string(JSON base GET "${spec}" ${dep} baseUrl)
  string(JSON name GET "${spec}" ${dep} files ${PLATFORM} name)
  string(JSON sha GET "${spec}" ${dep} files ${PLATFORM} sha256)
  set(have "")
  if(EXISTS "${DEPS_DIR}/${dep}/.sha256")
    file(READ "${DEPS_DIR}/${dep}/.sha256" have)
  endif()
  if(NOT have STREQUAL sha)
    download("${base}/${name}" ${name} ${sha})
    message(STATUS "Extracting ${name}")
    extract(${name} "${DEPS_DIR}/${dep}")
    file(REMOVE "${DEPS_DIR}/downloads/${name}")
    file(WRITE "${DEPS_DIR}/${dep}/.sha256" "${sha}")
  endif()
endforeach()

string(JSON obs_version GET "${spec}" obs-studio version)
string(JSON url GET "${spec}" obs-studio url)
string(JSON sha GET "${spec}" obs-studio sha256)
download("${url}" obs-studio-${obs_version}.tar.gz ${sha})
message(STATUS "Extracting OBS ${obs_version}")
extract(obs-studio-${obs_version}.tar.gz "${DEPS_DIR}/src")
set(obs_src "${DEPS_DIR}/src/obs-studio-${obs_version}")
set(obs_build "${DEPS_DIR}/src/build")
set(prefix_path "${DEPS_DIR}/prebuilt;${DEPS_DIR}/qt6")

# platform_arg goes in quoted: a list in an unquoted argument would be split.
if(PLATFORM STREQUAL "macos")
  set(generator -G Xcode -DCMAKE_OSX_DEPLOYMENT_TARGET=12.0)
  set(platform_arg "-DCMAKE_OSX_ARCHITECTURES=arm64;x86_64")
else()
  if(PLATFORM STREQUAL "windows-x64")
    set(arch x64)
  else()
    set(arch ARM64)
  endif()
  # The newest Visual Studio there and its Windows SDK.
  set(generator -A "${arch}")
  set(platform_arg "-DCMAKE_SYSTEM_VERSION=10.0")
endif()

# Only libobs and obs-frontend-api: no plugins, no frontend, no scripting.
message(STATUS "Configuring OBS ${obs_version}")
execute_process(
  COMMAND "${CMAKE_COMMAND}" -S "${obs_src}" -B "${obs_build}" ${generator} "${platform_arg}"
          -DOBS_CMAKE_VERSION:STRING=3.0.0 -DENABLE_PLUGINS:BOOL=OFF -DENABLE_FRONTEND:BOOL=OFF
          -DENABLE_UI:BOOL=OFF -DENABLE_SCRIPTING:BOOL=OFF
          -DOBS_VERSION_OVERRIDE:STRING=${obs_version} "-DCMAKE_PREFIX_PATH=${prefix_path}"
  OUTPUT_FILE "${DEPS_DIR}/obs-configure.log"
  ERROR_FILE "${DEPS_DIR}/obs-configure.log"
  RESULT_VARIABLE rc)
if(NOT rc EQUAL 0)
  file(READ "${DEPS_DIR}/obs-configure.log" log)
  message(FATAL_ERROR "configuring OBS failed:\n${log}")
endif()

message(STATUS "Building libobs and obs-frontend-api")
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${obs_build}" --target obs-frontend-api --config Release --parallel
  OUTPUT_FILE "${DEPS_DIR}/obs-build.log"
  ERROR_FILE "${DEPS_DIR}/obs-build.log"
  RESULT_VARIABLE rc)
if(NOT rc EQUAL 0)
  file(STRINGS "${DEPS_DIR}/obs-build.log" errors REGEX "error")
  list(JOIN errors "\n" errors)
  message(FATAL_ERROR "building OBS failed:\n${errors}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" --install "${obs_build}" --component Development --config Release
          --prefix "${DEPS_DIR}/prefix"
  OUTPUT_QUIET
  COMMAND_ERROR_IS_FATAL ANY)

# The sources, the build tree and obs-deps' libraries (FFmpeg & co., only
# libobs links them) are no longer needed and would bloat the CI cache.
file(REMOVE_RECURSE "${DEPS_DIR}/src" "${DEPS_DIR}/prebuilt")
file(REMOVE "${DEPS_DIR}/downloads/obs-studio-${obs_version}.tar.gz")

# On macOS libobs is a framework: its package config sits in it.
set(found_path "${DEPS_DIR}/prefix;${DEPS_DIR}/prefix/Frameworks;${DEPS_DIR}/qt6")
file(WRITE "${DEPS_DIR}/prefix-path.txt" "${found_path}")
file(WRITE "${stamp}" "${spec_hash}")
message(STATUS "OBS dependencies ready in ${DEPS_DIR}")
