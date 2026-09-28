# Builds the extraction and typed-resolution engine as a static library.
#
# The engine is third-party source under our maintenance. It is not developed here:
# new capability goes into Rust, and changes to these sources are limited to the
# vendoring scripts' renaming, the strip list, the interface layer in api/, and
# patches under patches/.
#
# Two dialects: C17 for the engine, and C++17 for the single translation unit that
# does macro preprocessing, whose entry points are exported with C linkage.

cmake_minimum_required(VERSION 3.25)
project(pdxe C CXX)

set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Strict aliasing is disabled because these sources predate its enforcement and the
# parser tables in particular alias freely. This is the upstream's own assumption,
# and honouring it is cheaper than auditing a quarter of a million lines.
set(PDXE_COMMON_FLAGS -fno-strict-aliasing)

# Warnings are fatal for the interface layer, which we write. They are visible but
# not fatal for the vendored sources: those are not ours to reshape, and a compiler
# upgrade must not be able to stop the build of code nobody here edits. Turn this on
# to see the vendored tree held to the same bar.
option(PDXE_VENDORED_WERROR "Treat warnings in vendored sources as errors" OFF)

set(PDXE_ROOT ${CMAKE_CURRENT_SOURCE_DIR})

# --- source sets -----------------------------------------------------------

file(GLOB PDXE_CORE_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/*.c)
file(GLOB PDXE_FOUNDATION_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/foundation/*.c)
file(GLOB PDXE_RESOLVER_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/lsp/*.c)
file(GLOB PDXE_RESOLVER_TABLES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/lsp/generated/*.c)

# The cross-file resolution sources call into a graph store this project does not
# vendor; the interface layer serves those calls instead. They join the build
# together with that layer, in the task that writes it.
file(GLOB PDXE_CROSS_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/resolve/*.c)

# The interface layer: ours, and held to the strict flags.
file(GLOB PDXE_API_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/api/*.c)

set(PDXE_VENDORED_SOURCES
  ${PDXE_ROOT}/vendored/ts_runtime/src/lib.c
  ${PDXE_ROOT}/vendored/yyjson/yyjson.c
  # The allocator, built as the single translation unit it provides for the purpose.
  ${PDXE_ROOT}/vendored/mimalloc/src/static.c
)

# --- grammars --------------------------------------------------------------
#
# The parser tables are not compiled directly. Each language has a wrapper in the
# engine sources that includes its parser and scanner, which is how they are kept
# apart: the generated tables declare static symbols of the same names, so compiling two
# of them into one translation unit would collide. Compiling the tables here as well
# would define every symbol twice.
#
# Their warnings are silenced at the wrapper, since the tables are generated and
# nobody will act on them.

# --- the library -----------------------------------------------------------

add_library(pdxe STATIC
  ${PDXE_CORE_SOURCES}
  ${PDXE_FOUNDATION_SOURCES}
  ${PDXE_RESOLVER_SOURCES}
  ${PDXE_RESOLVER_TABLES}
  ${PDXE_VENDORED_SOURCES}
  ${PDXE_API_SOURCES}
  ${PDXE_ROOT}/src/preprocessor.cpp
  ${PDXE_ROOT}/vendored/simplecpp/simplecpp.cpp
)

target_include_directories(pdxe PUBLIC
  ${PDXE_ROOT}
  ${PDXE_ROOT}/include
  ${PDXE_ROOT}/src
  # Deliberately not src/lsp or src/foundation. The sources reach those through
  # their directory prefix, and the foundation carries headers whose names match
  # the standard library's; putting it on the search path lets one of ours shadow a
  # system header for every file compiled, including the grammars.
  ${PDXE_ROOT}/vendored/ts_runtime/include
  ${PDXE_ROOT}/vendored/ts_runtime/src
  ${PDXE_ROOT}/vendored/ts_runtime/src/unicode
  ${PDXE_ROOT}/vendored
  ${PDXE_ROOT}/vendored/yyjson
  ${PDXE_ROOT}/vendored/mimalloc/include
  ${PDXE_ROOT}/vendored/verstable
  ${PDXE_ROOT}/vendored/wyhash
  ${PDXE_ROOT}/vendored/common
)

target_compile_options(pdxe PRIVATE ${PDXE_COMMON_FLAGS} -Wall -Wextra)

# The grammar wrappers are generated parser tables by inclusion: silence them.
file(GLOB PDXE_GRAMMAR_WRAPPERS CONFIGURE_DEPENDS ${PDXE_ROOT}/src/grammar_*.c)
if(PDXE_GRAMMAR_WRAPPERS)
  set_source_files_properties(${PDXE_GRAMMAR_WRAPPERS} PROPERTIES COMPILE_OPTIONS "-w")
endif()
if(PDXE_VENDORED_WERROR)
  target_compile_options(pdxe PRIVATE -Werror)
endif()

find_package(Threads REQUIRED)
target_link_libraries(pdxe PUBLIC Threads::Threads)
if(UNIX AND NOT APPLE)
  target_link_libraries(pdxe PUBLIC m)
endif()

set_target_properties(pdxe PROPERTIES
  ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}
  OUTPUT_NAME pdxe
)
