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

# Warnings are errors throughout, as the toolchain specification requires. For the
# vendored sources a few classes are exempt below, named and kept visible: the
# reference compiles its typed-resolution layer with every warning suppressed, and
# what that layer carries is dead code and style, not defects. Nothing is exempt in
# the interface layer, which is ours. The switch exists only to let a compiler newer
# than the ones this is tested with finish a build while its new warnings are
# looked at; CI never turns it off.
option(PDXE_VENDORED_WERROR "Treat warnings in vendored sources as errors" ON)

set(PDXE_ROOT ${CMAKE_CURRENT_SOURCE_DIR})

# The exact flags each source is built with, for tests that compile probes the same
# way (tests/warning_scope.py) and for editors.
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# --- source sets -----------------------------------------------------------

file(GLOB PDXE_CORE_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/*.c)
# The unity file lives among them but is compiled as the resolver layer, below.
list(REMOVE_ITEM PDXE_CORE_SOURCES ${PDXE_ROOT}/src/lsp_all.c)
file(GLOB PDXE_FOUNDATION_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/foundation/*.c)
# The typed resolution layer is one translation unit, compiled through its unity
# file, never file by file. Its sources rely on being compiled together in a set
# order: generated tables define macros that switch off fallback definitions, and a
# file-by-file build defines both, so a program linking them may silently get the
# fallback. Everything under lsp/ is reached from the unity file.
set(PDXE_RESOLVER_SOURCES ${PDXE_ROOT}/src/lsp_all.c)
set(PDXE_RESOLVER_TABLES "")

# The cross-file resolution sources call into a graph store this project does not
# vendor; the interface layer serves those calls instead. They join the build
# together with that layer, in the task that writes it.
file(GLOB PDXE_CROSS_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/src/resolve/*.c)

# The interface layer: ours, and held to the strict flags.
file(GLOB PDXE_API_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/api/*.c)

# The parser runtime and the macro preprocessor are not listed: each is included by
# a wrapper among the engine sources (src/ts_runtime.c, src/preprocessor.cpp), and
# compiling them here as well defined every one of their symbols twice.
# The allocator's implementation is not among them: nothing the interface reaches
# calls it (scripts/vendor/strip-list.txt), and only its header, for types, is kept.
set(PDXE_VENDORED_SOURCES
  ${PDXE_ROOT}/vendored/yyjson/yyjson.c
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
  ${PDXE_CROSS_SOURCES}
  ${PDXE_VENDORED_SOURCES}
  ${PDXE_API_SOURCES}
  ${PDXE_ROOT}/src/preprocessor.cpp
)

target_include_directories(pdxe PUBLIC
  ${PDXE_ROOT}
  ${PDXE_ROOT}/include
  ${PDXE_ROOT}/api
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
  ${PDXE_ROOT}/vendored/common
)

target_compile_options(pdxe PRIVATE ${PDXE_COMMON_FLAGS} -Wall -Wextra)

# On Windows the C runtime marks the POSIX names the engine uses (strdup and the
# like) and its unchecked string functions as deprecated. They are what the sources
# are written against, so the deprecation is switched off rather than the vendored
# code rewritten.
if(WIN32)
  target_compile_definitions(pdxe PRIVATE _CRT_SECURE_NO_WARNINGS _CRT_NONSTDC_NO_DEPRECATE)
endif()

# Microsoft's C runtime lacks some POSIX names and headers the vendored sources use
# outside their Windows branches; the reference builds for Windows against a runtime
# that has them. api/windows supplies the names (a header included ahead of every
# source) and stands in for the missing headers, so the sources compile unchanged.
if(WIN32 AND NOT MINGW)
  target_include_directories(pdxe PRIVATE ${PDXE_ROOT}/api/windows)
  target_compile_options(pdxe PRIVATE
    "SHELL:-include \"${PDXE_ROOT}/api/windows/posix_names.h\"")
endif()

# Some diagnostics are defects wherever they occur, vendored or not: a function
# called without a declaration is assumed to return int, which truncates a pointer on
# a 64-bit target, and the conversions below corrupt values silently. These are
# errors always, whatever PDXE_VENDORED_WERROR says.
target_compile_options(pdxe PRIVATE
  $<$<COMPILE_LANGUAGE:C>:-Werror=implicit-function-declaration>
  $<$<COMPILE_LANGUAGE:C>:-Werror=int-conversion>
  $<$<COMPILE_LANGUAGE:C>:-Werror=incompatible-pointer-types>
  $<$<COMPILE_LANGUAGE:C>:-Werror=return-type>)

# The interface layer is ours, so its warnings are errors regardless of the switch
# above: that switch exists for code we did not write.
if(PDXE_API_SOURCES)
  set_source_files_properties(${PDXE_API_SOURCES} PROPERTIES
    COMPILE_OPTIONS "-Wall;-Wextra;-Werror")
endif()

# The grammar wrappers are generated parser tables by inclusion: silence them.
file(GLOB PDXE_GRAMMAR_WRAPPERS CONFIGURE_DEPENDS ${PDXE_ROOT}/src/grammar_*.c)
if(PDXE_GRAMMAR_WRAPPERS)
  set_source_files_properties(${PDXE_GRAMMAR_WRAPPERS} PROPERTIES COMPILE_OPTIONS "-w")
endif()
# Every source that is not the interface layer: the vendored engine, its libraries
# and the preprocessor. The exemptions below attach to these files and nothing else.
set(PDXE_VENDORED_ALL
  ${PDXE_CORE_SOURCES} ${PDXE_FOUNDATION_SOURCES} ${PDXE_RESOLVER_SOURCES}
  ${PDXE_CROSS_SOURCES} ${PDXE_VENDORED_SOURCES} ${PDXE_ROOT}/src/preprocessor.cpp)

if(PDXE_VENDORED_WERROR)
  target_compile_options(pdxe PRIVATE -Werror)

  # Exempt in vendored code only: warnings that report code nobody calls, a parameter
  # or value nobody reads, a comment that nests a comment opener, and a sign mismatch
  # in a comparison. They stay visible in the build log; they are not errors. Set on
  # the vendored files rather than the target, because a specific -Wno-error outranks
  # a general -Werror wherever both appear, so on the target it would reach the
  # interface layer too. tests/warning_scope.py checks it does not.
  include(CheckCCompilerFlag)
  set(PDXE_VENDORED_EXEMPT
    unused-function unused-parameter unused-variable unused-but-set-variable
    unused-const-variable unused-value comment sign-compare
    # Bounded writes into fixed buffers, truncating by design; the reference exempts
    # this class for the same reason.
    format-truncation)
  foreach(warning ${PDXE_VENDORED_EXEMPT})
    string(MAKE_C_IDENTIFIER "PDXE_HAS_W_${warning}" flag_var)
    check_c_compiler_flag(-W${warning} ${flag_var})
    if(${flag_var})
      set_property(SOURCE ${PDXE_VENDORED_ALL} APPEND PROPERTY
        COMPILE_OPTIONS $<$<COMPILE_LANGUAGE:C>:-Wno-error=${warning}>)
    endif()
  endforeach()

  # One file, one false positive: gcc reports a freshly allocated block, passed on as
  # a const pointer so its usable size can be asked for, as possibly uninitialised,
  # though nothing reads what it points to. The class stays an error everywhere else.
  check_c_compiler_flag(-Wmaybe-uninitialized PDXE_HAS_W_MAYBE_UNINITIALIZED)
  if(PDXE_HAS_W_MAYBE_UNINITIALIZED)
    set_property(SOURCE ${PDXE_ROOT}/src/foundation/mem_core.c APPEND PROPERTY
      COMPILE_OPTIONS -Wno-error=maybe-uninitialized)
  endif()
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

# --- the interface's own tests ---------------------------------------------
#
# Small C programs that exercise the interface exactly as a caller does, linked
# against the archive. They see the internal headers too, which a caller does not:
# some tests compare what the resolver built with what the reference builds.
option(PDXE_BUILD_TESTS "Build the interface's test programs" OFF)
if(PDXE_BUILD_TESTS)
  enable_testing()
  file(GLOB PDXE_TEST_SOURCES CONFIGURE_DEPENDS ${PDXE_ROOT}/tests/*.c)
  foreach(test_source ${PDXE_TEST_SOURCES})
    get_filename_component(test_name ${test_source} NAME_WE)
    add_executable(${test_name} ${test_source})
    target_include_directories(${test_name} PRIVATE $<TARGET_PROPERTY:pdxe,INCLUDE_DIRECTORIES>)
    target_compile_options(${test_name} PRIVATE -Wall -Wextra -Werror)
    target_link_libraries(${test_name} PRIVATE pdxe)
    # The archive holds C++ (the preprocessor), so the C++ runtime must be linked.
    set_target_properties(${test_name} PROPERTIES
      LINKER_LANGUAGE CXX
      RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/tests)
  endforeach()

  set(PDXE_SMOKE_FIXTURES ${PDXE_ROOT}/tests/fixtures/smoke)
  set(PDXE_RESOLVE_FIXTURE_ROOT ${PDXE_ROOT}/tests/fixtures/resolve)
  add_test(NAME abi_smoke COMMAND abi_smoke ${PDXE_SMOKE_FIXTURES})
  add_test(NAME abi_result_build_roundtrip
    COMMAND abi_result_build_roundtrip ${PDXE_SMOKE_FIXTURES} ${PDXE_RESOLVE_FIXTURE_ROOT})
  file(GLOB PDXE_RESOLVE_FIXTURE_DIRS LIST_DIRECTORIES true ${PDXE_RESOLVE_FIXTURE_ROOT}/*)
  add_test(NAME abi_no_output
    COMMAND abi_no_output ${PDXE_SMOKE_FIXTURES} ${PDXE_RESOLVE_FIXTURE_DIRS})

  # The warning exemptions reach vendored sources and never the interface layer.
  find_package(Python3 REQUIRED COMPONENTS Interpreter)
  add_test(NAME api_warnings_are_errors
    COMMAND ${Python3_EXECUTABLE} ${PDXE_ROOT}/tests/warning_scope.py ${CMAKE_BINARY_DIR})

  # Typed resolution, one test per fixture, named after it: our answers against the
  # reference's recorded ones, the fixture's own assertions, and independence from
  # the order files are added. See scripts/engine/typed-differential.py.
  file(GLOB PDXE_RESOLVE_FIXTURES LIST_DIRECTORIES true ${PDXE_ROOT}/tests/fixtures/resolve/*)
  foreach(fixture ${PDXE_RESOLVE_FIXTURES})
    if(IS_DIRECTORY ${fixture})
      get_filename_component(fixture_name ${fixture} NAME)
      add_test(NAME ${fixture_name}
        COMMAND ${Python3_EXECUTABLE} ${PDXE_ROOT}/../scripts/engine/typed-differential.py
                --dump $<TARGET_FILE:resolve_dump> ${fixture})
    endif()
  endforeach()
endif()
