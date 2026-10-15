# The FM-1 plugin's AU (and later VST3), by clap-wrapper (MIT) around the CLAP that cargo builds.
# clap-wrapper downloads the SDKs it needs: the CLAP SDK (MIT), Apple's AudioUnitSDK (Apache-2.0)
# and, for VST3, Steinberg's VST3 SDK (MIT since 3.8). tools/build_plugin.sh drives it:
#   cmake -B build -DFM1_CLAP=/path/to/FM-1.clap && cmake --build build
cmake_minimum_required(VERSION 3.21)
set(CMAKE_OSX_DEPLOYMENT_TARGET 11.0 CACHE STRING "")
set(CMAKE_OSX_ARCHITECTURES arm64 CACHE STRING "")
project(fm1-plugin-wrap VERSION 0.1.0 LANGUAGES C CXX OBJC OBJCXX)
set(CMAKE_CXX_STANDARD 17)

set(FM1_CLAP "" CACHE PATH "The FM-1.clap bundle the wrappers carry")
if (NOT EXISTS "${FM1_CLAP}")
    message(FATAL_ERROR "FM1_CLAP (${FM1_CLAP}) is not a .clap bundle: run tools/build_plugin.sh")
endif ()

set(CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES TRUE CACHE BOOL "" FORCE)
include(FetchContent)
FetchContent_Declare(clap-wrapper
        GIT_REPOSITORY https://github.com/free-audio/clap-wrapper.git
        GIT_TAG v0.16.0)
FetchContent_MakeAvailable(clap-wrapper)

# An instrument ('aumu'); the codes name it to AU hosts: manufacturer Fm1e, subtype Fm1s.
add_library(fm1_auv2 MODULE)
target_add_auv2_wrapper(
        TARGET fm1_auv2
        OUTPUT_NAME "FM-1"
        BUNDLE_IDENTIFIER "io.github.joakimeriksson.fm1-emu.auv2"
        BUNDLE_VERSION "${PROJECT_VERSION}"
        MANUFACTURER_NAME "fm1-emu"
        MANUFACTURER_CODE "Fm1e"
        SUBTYPE_CODE "Fm1s"
        INSTRUMENT_TYPE "aumu"
        MACOS_EMBEDDED_CLAP_LOCATION "${FM1_CLAP}")

# The VST3 (Steinberg's SDK, MIT since 3.8), the same CLAP inside.
add_library(fm1_vst3 MODULE)
target_add_vst3_wrapper(
        TARGET fm1_vst3
        OUTPUT_NAME "FM-1"
        BUNDLE_IDENTIFIER "io.github.joakimeriksson.fm1-emu.vst3"
        BUNDLE_VERSION "${PROJECT_VERSION}"
        MACOS_EMBEDDED_CLAP_LOCATION "${FM1_CLAP}")
