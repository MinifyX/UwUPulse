# The UwUPulse OBS plugin. Built by scripts/build.mjs, which prepares what
# this needs:
#   CMAKE_PREFIX_PATH       libobs, obs-frontend-api, Qt 6 (cmake/deps.cmake)
#   OBS_BROWSER_INCLUDE     folder with obs-browser's browser-panel.hpp
#   UWUPULSE_CORE_LIB       the Rust core as a static library (crates/uwupulse-obs)
#   UWUPULSE_CORE_LINK      the system libraries it needs (rustc's native-static-libs)
#   UWUPULSE_UI_DIR         the built frontend (apps/desktop/dist)
# `cmake --install` lays the plugin out in CMAKE_INSTALL_PREFIX like OBS
# expects it in its plugins folder: uwupulse.plugin on macOS, uwupulse/bin/64bit
# and uwupulse/data on Windows and Linux.

cmake_minimum_required(VERSION 3.28)

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../../Cargo.toml" _cargo)
string(REGEX MATCH "\nversion = \"([0-9.]+)\"" _ "${_cargo}")
set(UWUPULSE_VERSION "${CMAKE_MATCH_1}")

project(uwupulse VERSION ${UWUPULSE_VERSION} LANGUAGES C CXX)

set(UWUPULSE_CORE_LIB "" CACHE FILEPATH "Rust core static library")
set(UWUPULSE_CORE_LINK "" CACHE STRING "System libraries of the Rust core")
set(UWUPULSE_UI_DIR "" CACHE PATH "Built frontend")
set(OBS_BROWSER_INCLUDE "" CACHE PATH "Folder with panel/browser-panel.hpp")
foreach(var UWUPULSE_CORE_LIB UWUPULSE_UI_DIR OBS_BROWSER_INCLUDE)
  if(NOT EXISTS "${${var}}")
    message(FATAL_ERROR "${var} is missing (run plugins/obs/scripts/build.mjs)")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)

find_package(libobs REQUIRED)
find_package(obs-frontend-api REQUIRED)
find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets)
# Qt asks for AGL, which newer macOS SDKs no longer have; nothing here uses it.
if(APPLE AND TARGET WrapOpenGL::WrapOpenGL)
  get_target_property(_gl WrapOpenGL::WrapOpenGL INTERFACE_LINK_LIBRARIES)
  list(FILTER _gl EXCLUDE REGEX "AGL")
  set_target_properties(WrapOpenGL::WrapOpenGL PROPERTIES INTERFACE_LINK_LIBRARIES "${_gl}")
endif()

add_library(uwupulse MODULE src/plugin.cpp src/dock.cpp src/source.cpp)
target_include_directories(uwupulse PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/uwupulse-obs/include"
                                            "${OBS_BROWSER_INCLUDE}/panel")
target_link_libraries(uwupulse PRIVATE OBS::libobs OBS::obs-frontend-api Qt6::Core Qt6::Gui Qt6::Widgets
                                       "${UWUPULSE_CORE_LIB}" ${UWUPULSE_CORE_LINK})
set_target_properties(uwupulse PROPERTIES PREFIX "")

if(MSVC)
  target_compile_options(uwupulse PRIVATE /W3 /utf-8 /MP)
  # The Rust core is built against the release CRT, like OBS.
  set_property(TARGET uwupulse PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")
else()
  target_compile_options(uwupulse PRIVATE -Wall -Wextra -Wno-unused-parameter)
endif()

# Only OBS's module entry points are exported: nothing from the Rust core
# (sqlite, tokio, ...) can meet another plugin's copy.
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
  target_compile_definitions(uwupulse PRIVATE ENABLE_WAYLAND)
  target_link_options(uwupulse PRIVATE "LINKER:--exclude-libs,ALL" "LINKER:--as-needed" "LINKER:--gc-sections"
                      "LINKER:--version-script=${CMAKE_CURRENT_SOURCE_DIR}/cmake/exports.map")
elseif(APPLE)
  target_link_options(uwupulse PRIVATE "LINKER:-dead_strip"
                      "LINKER:-exported_symbols_list,${CMAKE_CURRENT_SOURCE_DIR}/cmake/exports.txt")
endif()

if(APPLE)
  set_target_properties(
    uwupulse
    PROPERTIES BUNDLE TRUE
               BUNDLE_EXTENSION plugin
               MACOSX_BUNDLE_INFO_PLIST "${CMAKE_CURRENT_SOURCE_DIR}/cmake/Info.plist.in"
               MACOSX_BUNDLE_GUI_IDENTIFIER "app.uwupulse.obs"
               MACOSX_BUNDLE_BUNDLE_VERSION "${UWUPULSE_VERSION}"
               MACOSX_BUNDLE_SHORT_VERSION_STRING "${UWUPULSE_VERSION}")
  install(TARGETS uwupulse LIBRARY DESTINATION .)
  set(data_dir "uwupulse.plugin/Contents/Resources")
else()
  install(TARGETS uwupulse LIBRARY DESTINATION uwupulse/bin/64bit RUNTIME DESTINATION uwupulse/bin/64bit)
  set(data_dir "uwupulse/data")
endif()
install(DIRECTORY data/locale DESTINATION "${data_dir}")
install(DIRECTORY "${UWUPULSE_UI_DIR}/" DESTINATION "${data_dir}/ui")
