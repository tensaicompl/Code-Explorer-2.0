/*
 * Vendored subset: the language of a file by its name and extension, extracted
 * verbatim from the reference's language source by
 * scripts/vendor/extract-functions.py, with the tables it reads. Telling languages
 * apart by reading a file's content, which the rest of that source does, is
 * discovery, and is deliberately not here.
 */

#include "discover/discover.h"
#include "discover/userconfig.h"
#include "pdxe_core.h" // PDXELanguage, PDXE_LANG_*
#include "foundation/constants.h"
#define SLEN(s) (sizeof(s) - 1)
#include <ctype.h>
#include <stdio.h>
#include <string.h>
#define EXT_TABLE_SIZE (sizeof(EXT_TABLE) / sizeof(EXT_TABLE[0]))
#define FILENAME_TABLE_SIZE (sizeof(FILENAME_TABLE) / sizeof(FILENAME_TABLE[0]))

typedef struct {
    const char *ext; /* including dot, e.g. ".go" */
    PDXELanguage language;
} ext_entry_t;

/* Sorted by extension for binary search (but linear scan is fine for ~120 entries) */
static const ext_entry_t EXT_TABLE[] = {
    /* Bash */
    {".bash", PDXE_LANG_BASH},
    {".sh", PDXE_LANG_BASH},

    /* C */
    {".c", PDXE_LANG_C},

    /* C++ */
    {".cc", PDXE_LANG_CPP},
    {".ccm", PDXE_LANG_CPP},
    {".cpp", PDXE_LANG_CPP},
    {".cppm", PDXE_LANG_CPP},
    {".cxx", PDXE_LANG_CPP},
    {".h", PDXE_LANG_CPP},
    {".hh", PDXE_LANG_CPP},
    {".hpp", PDXE_LANG_CPP},
    {".hxx", PDXE_LANG_CPP},
    {".ixx", PDXE_LANG_CPP},

    /* C# */
    {".cs", PDXE_LANG_CSHARP},
    /* Blazor components. The C# grammar recovers the @code block; the
     * surrounding markup parses as ERROR regions and is reported via
     * parse_partial, which is why this is a best-effort mapping rather
     * than a dedicated grammar. */
    {".razor", PDXE_LANG_CSHARP},
    /* Razor Pages / MVC views. Same Razor syntax and the same C# host as
     * .razor, and equally unmapped before this: an ASP.NET Core app's views
     * produced no nodes at all. The `@page` directive that defines a Razor
     * Page lives in this file type, so the route extraction below matters
     * more here than it does for components. Best-effort on the same terms:
     * the C# grammar recovers the @{ } / @functions blocks, the surrounding
     * markup lands in ERROR regions and is reported via parse_partial. */
    {".cshtml", PDXE_LANG_CSHARP},

    /* Clojure */
    {".clj", PDXE_LANG_CLOJURE},
    {".cljc", PDXE_LANG_CLOJURE},
    {".cljs", PDXE_LANG_CLOJURE},

    /* CMake */
    {".cmake", PDXE_LANG_CMAKE},

    /* COBOL */
    {".cbl", PDXE_LANG_COBOL},
    {".cob", PDXE_LANG_COBOL},

    /* Common Lisp */
    {".cl", PDXE_LANG_COMMONLISP},
    {".lisp", PDXE_LANG_COMMONLISP},
    {".lsp", PDXE_LANG_COMMONLISP},

    /* CSS */
    {".css", PDXE_LANG_CSS},

    /* CUDA */
    {".cu", PDXE_LANG_CUDA},
    {".cuh", PDXE_LANG_CUDA},

    /* Dart */
    {".dart", PDXE_LANG_DART},

    /* Dockerfile */
    {".dockerfile", PDXE_LANG_DOCKERFILE},

    /* Elixir */
    {".ex", PDXE_LANG_ELIXIR},
    {".exs", PDXE_LANG_ELIXIR},

    /* DotEnv */
    {".env", PDXE_LANG_DOTENV},

    /* Elm */
    {".elm", PDXE_LANG_ELM},

    /* ArkTS (HarmonyOS/OpenHarmony) */
    {".ets", PDXE_LANG_ARKTS},

    /* Emacs Lisp */
    {".el", PDXE_LANG_EMACSLISP},

    /* Erlang */
    {".erl", PDXE_LANG_ERLANG},

    /* F# */
    {".fs", PDXE_LANG_FSHARP},
    {".fsi", PDXE_LANG_FSHARP},
    {".fsx", PDXE_LANG_FSHARP},

    /* FORM */
    {".frm", PDXE_LANG_FORM},
    {".prc", PDXE_LANG_FORM},

    /* Fortran */
    {".f03", PDXE_LANG_FORTRAN},
    {".f08", PDXE_LANG_FORTRAN},
    {".f90", PDXE_LANG_FORTRAN},
    {".f95", PDXE_LANG_FORTRAN},

    /* GLSL */
    {".frag", PDXE_LANG_GLSL},
    {".glsl", PDXE_LANG_GLSL},
    {".vert", PDXE_LANG_GLSL},

    /* Go */
    {".go", PDXE_LANG_GO},

    /* GraphQL */
    {".gql", PDXE_LANG_GRAPHQL},
    {".graphql", PDXE_LANG_GRAPHQL},

    /* Groovy */
    {".gradle", PDXE_LANG_GROOVY},
    {".groovy", PDXE_LANG_GROOVY},

    /* Haskell */
    {".hs", PDXE_LANG_HASKELL},

    /* HCL / Terraform */
    {".hcl", PDXE_LANG_HCL},
    {".tf", PDXE_LANG_HCL},

    /* HTML */
    {".htm", PDXE_LANG_HTML},
    {".html", PDXE_LANG_HTML},

    /* INI */
    {".cfg", PDXE_LANG_INI},
    {".conf", PDXE_LANG_INI},
    {".ini", PDXE_LANG_INI},

    /* Java */
    {".java", PDXE_LANG_JAVA},

    /* JavaScript */
    {".js", PDXE_LANG_JAVASCRIPT},
    {".jsx", PDXE_LANG_JAVASCRIPT},
    {".mjs", PDXE_LANG_JAVASCRIPT}, /* ES modules (#197) */
    {".cjs", PDXE_LANG_JAVASCRIPT}, /* CommonJS modules */

    /* JSON */
    {".json", PDXE_LANG_JSON},

    /* Julia */
    {".jl", PDXE_LANG_JULIA},

    /* Kotlin */
    {".kt", PDXE_LANG_KOTLIN},
    {".kts", PDXE_LANG_KOTLIN},

    /* Lean */
    {".lean", PDXE_LANG_LEAN},

    /* Lua */
    {".lua", PDXE_LANG_LUA},

    /* Magma */
    {".mag", PDXE_LANG_MAGMA},
    {".magma", PDXE_LANG_MAGMA},

    /* Makefile */
    {".mk", PDXE_LANG_MAKEFILE},

    /* Markdown */
    {".md", PDXE_LANG_MARKDOWN},
    {".mdx", PDXE_LANG_MARKDOWN},

    /* MATLAB */
    {".m", PDXE_LANG_MATLAB},
    {".matlab", PDXE_LANG_MATLAB},
    {".mlx", PDXE_LANG_MATLAB},

    /* Meson */
    {".meson", PDXE_LANG_MESON},

    /* Mojo */
    {".mojo", PDXE_LANG_MOJO},

    /* Nix */
    {".nix", PDXE_LANG_NIX},

    /* OCaml */
    {".ml", PDXE_LANG_OCAML},
    {".mli", PDXE_LANG_OCAML},

    /* Perl */
    {".pl", PDXE_LANG_PERL},
    {".pm", PDXE_LANG_PERL},

    /* PHP */
    {".php", PDXE_LANG_PHP},

    /* Oracle PL/SQL (do not map .sql — stays generic SQL; .prc stays FORM) */
    {".pks", PDXE_LANG_PLSQL},
    {".pkb", PDXE_LANG_PLSQL},
    {".pck", PDXE_LANG_PLSQL},
    {".pls", PDXE_LANG_PLSQL},
    {".plb", PDXE_LANG_PLSQL},
    {".plsql", PDXE_LANG_PLSQL},
    {".fnc", PDXE_LANG_PLSQL},
    {".trg", PDXE_LANG_PLSQL},
    {".bdy", PDXE_LANG_PLSQL},
    {".tps", PDXE_LANG_PLSQL},
    {".tpb", PDXE_LANG_PLSQL},

    /* Protobuf */
    {".proto", PDXE_LANG_PROTOBUF},

    /* Python */
    {".py", PDXE_LANG_PYTHON},

    /* R — case insensitive handled separately */
    {".R", PDXE_LANG_R},
    {".r", PDXE_LANG_R},

    /* Ruby */
    {".gemspec", PDXE_LANG_RUBY},
    {".rake", PDXE_LANG_RUBY},
    {".rb", PDXE_LANG_RUBY},

    /* Rust */
    {".rs", PDXE_LANG_RUST},

    /* Scala */
    {".sc", PDXE_LANG_SCALA},
    {".scala", PDXE_LANG_SCALA},

    /* SCSS */
    {".scss", PDXE_LANG_SCSS},

    /* SQL */
    {".sql", PDXE_LANG_SQL},

    /* Svelte */
    {".svelte", PDXE_LANG_SVELTE},

    /* Swift */
    {".swift", PDXE_LANG_SWIFT},

    /* SystemVerilog + Verilog */
    {".sv", PDXE_LANG_VERILOG},
    {".v", PDXE_LANG_VERILOG},

    /* TOML */
    {".toml", PDXE_LANG_TOML},

    /* TSX */
    {".tsx", PDXE_LANG_TSX},

    /* TypeScript */
    {".ts", PDXE_LANG_TYPESCRIPT},
    {".mts", PDXE_LANG_TYPESCRIPT}, /* TS ES modules */
    {".cts", PDXE_LANG_TYPESCRIPT}, /* TS CommonJS modules */

    /* VimScript */
    {".vim", PDXE_LANG_VIMSCRIPT},
    {".vimrc", PDXE_LANG_VIMSCRIPT},
    {"justfile", PDXE_LANG_JUST},
    {"Justfile", PDXE_LANG_JUST},
    {".justfile", PDXE_LANG_JUST},
    {".just", PDXE_LANG_JUST}, /* `import 'common.just'` target files */
    {"hyprland.conf", PDXE_LANG_HYPRLANG},
    {"ssh_config", PDXE_LANG_SSHCONFIG},
    {"sshd_config", PDXE_LANG_SSHCONFIG},
    {"BUILD", PDXE_LANG_STARLARK},
    {"BUILD.bazel", PDXE_LANG_STARLARK},
    {"WORKSPACE", PDXE_LANG_STARLARK},
    {"WORKSPACE.bazel", PDXE_LANG_STARLARK},

    /* BitBake include fragments — `require/include foo.inc` target files.
     * NOTE: .inc is also used by ObjectScript include (macro) files; the
     * ambiguity is resolved by content in pdxe_disambiguate_inc(). */
    {".inc", PDXE_LANG_BITBAKE},

    /* InterSystems ObjectScript routines (.mac/.int/.rtn unambiguous; .cls is
     * shared with Apex and resolved by content in pdxe_disambiguate_cls()). */
    {".mac", PDXE_LANG_OBJECTSCRIPT_ROUTINE},
    {".int", PDXE_LANG_OBJECTSCRIPT_ROUTINE},
    {".rtn", PDXE_LANG_OBJECTSCRIPT_ROUTINE},

    /* Vue */
    {".vue", PDXE_LANG_VUE},

    /* Wolfram */
    {".wl", PDXE_LANG_WOLFRAM},
    {".wls", PDXE_LANG_WOLFRAM},

    /* XML */
    {".xml", PDXE_LANG_XML},
    {".xsd", PDXE_LANG_XML},
    {".xsl", PDXE_LANG_XML},
    {".svg", PDXE_LANG_XML},
    /* MSBuild project system. Plain XML documents, and the files that carry a
     * .NET repository's package references, target frameworks, build hooks and
     * project layout — none of which were reachable while these were unmapped. */
    {".csproj", PDXE_LANG_XML},
    {".vbproj", PDXE_LANG_XML},
    {".fsproj", PDXE_LANG_XML},
    {".props", PDXE_LANG_XML},
    {".targets", PDXE_LANG_XML},
    {".nuspec", PDXE_LANG_XML},
    {".slnx", PDXE_LANG_XML},
    {".runsettings", PDXE_LANG_XML},
    /* .NET resource files: the localized strings a UI reads back by key. */
    {".resx", PDXE_LANG_XML},
    /* XAML views: WPF, WinUI, MAUI (.xaml) and Avalonia (.axaml). */
    {".xaml", PDXE_LANG_XML},
    {".axaml", PDXE_LANG_XML},
    /* Application manifests: Apple property lists and privacy manifests, Win32
     * side-by-side manifests and MSIX packages. */
    {".plist", PDXE_LANG_XML},
    {".xcprivacy", PDXE_LANG_XML},
    {".manifest", PDXE_LANG_XML},
    {".appxmanifest", PDXE_LANG_XML},

    /* YAML */
    {".yaml", PDXE_LANG_YAML},
    {".yml", PDXE_LANG_YAML},

    /* Ada */
    {".adb", PDXE_LANG_ADA},

    /* Ada */
    {".ads", PDXE_LANG_ADA},

    /* Agda */
    {".agda", PDXE_LANG_AGDA},

    /* Astro */
    {".astro", PDXE_LANG_ASTRO},

    /* AWK */
    {".awk", PDXE_LANG_AWK},

    /* BitBake */
    {".bb", PDXE_LANG_BITBAKE},

    /* BitBake */
    {".bbappend", PDXE_LANG_BITBAKE},

    /* BitBake */
    {".bbclass", PDXE_LANG_BITBAKE},

    /* Beancount */
    {".beancount", PDXE_LANG_BEANCOUNT},

    /* BibTeX */
    {".bib", PDXE_LANG_BIBTEX},

    /* Bicep */
    {".bicep", PDXE_LANG_BICEP},

    /* Blade */
    /* .blade.php handled by userconfig compound extensions, not EXT_TABLE */

    /* Starlark */
    {".bzl", PDXE_LANG_STARLARK},

    /* Cairo */
    {".cairo", PDXE_LANG_CAIRO},

    /* Cap'n Proto */
    {".capnp", PDXE_LANG_CAPNP},

    /* Apex */
    {".cls", PDXE_LANG_APEX},

    /* Crystal */
    {".cr", PDXE_LANG_CRYSTAL},

    /* CSV */
    {".csv", PDXE_LANG_CSV},

    /* D */
    {".d", PDXE_LANG_DLANG},

    /* Diff */
    {".diff", PDXE_LANG_DIFF},

    /* Pascal */
    {".dpr", PDXE_LANG_PASCAL},

    /* DeviceTree */
    {".dts", PDXE_LANG_DEVICETREE},

    /* DeviceTree */
    {".dtsi", PDXE_LANG_DEVICETREE},

    /* FunC */
    {".fc", PDXE_LANG_FUNC},

    /* Fish */
    {".fish", PDXE_LANG_FISH},

    /* Fennel */
    {".fnl", PDXE_LANG_FENNEL},

    /* HLSL */
    {".fx", PDXE_LANG_HLSL},

    /* GDScript */
    {".gd", PDXE_LANG_GDSCRIPT},

    /* Gleam */
    {".gleam", PDXE_LANG_GLEAM},

    /* GN */
    {".gn", PDXE_LANG_GN},

    /* GN */
    {".gni", PDXE_LANG_GN},

    /* Go Template */
    {".gotmpl", PDXE_LANG_GOTEMPLATE},
    {".tpl", PDXE_LANG_GOTEMPLATE}, /* Helm _helpers.tpl named-template definitions */

    /* Hare */
    {".ha", PDXE_LANG_HARE},

    /* Hyprlang */
    {".hl", PDXE_LANG_HYPRLANG},

    /* HLSL */
    {".hlsl", PDXE_LANG_HLSL},

    /* HLSL */
    {".hlsli", PDXE_LANG_HLSL},

    /* ISPC */
    {".ispc", PDXE_LANG_ISPC},

    /* Jinja2 */
    {".j2", PDXE_LANG_JINJA2},

    /* Janet */
    {".janet", PDXE_LANG_JANET},

    /* Jinja2 */
    {".jinja", PDXE_LANG_JINJA2},

    /* Jinja2 */
    {".jinja2", PDXE_LANG_JINJA2},

    /* JSON5 */
    {".json5", PDXE_LANG_JSON5},

    /* Jsonnet */
    {".jsonnet", PDXE_LANG_JSONNET},

    /* KDL */
    {".kdl", PDXE_LANG_KDL},

    /* Linker Script */
    {".ld", PDXE_LANG_LINKERSCRIPT},

    /* Linker Script */
    {".lds", PDXE_LANG_LINKERSCRIPT},

    /* Jsonnet */
    {".libsonnet", PDXE_LANG_JSONNET},

    /* Liquid */
    {".liquid", PDXE_LANG_LIQUID},

    /* LLVM IR */
    {".ll", PDXE_LANG_LLVM_IR},

    /* Pascal */
    {".lpr", PDXE_LANG_PASCAL},

    /* Luau */
    {".luau", PDXE_LANG_LUAU},

    /* Qt QML */
    {".qml", PDXE_LANG_QML},

    /* CFML / ColdFusion — .cfm are tag templates; .cfc components may be EITHER
     * script-dialect (component { ... }) or tag-dialect (<cfcomponent> ...). The
     * table default is script; tag-based .cfc are resolved by content in
     * pdxe_disambiguate_cfc(). */
    {".cfc", PDXE_LANG_CFSCRIPT},
    {".cfm", PDXE_LANG_CFML},

    /* Mermaid */
    {".mermaid", PDXE_LANG_MERMAID},

    /* Mermaid */
    {".mmd", PDXE_LANG_MERMAID},

    /* Move */
    {".move", PDXE_LANG_MOVE},

    /* NASM */
    {".nasm", PDXE_LANG_NASM},

    /* Nickel */
    {".ncl", PDXE_LANG_NICKEL},

    /* Nim */

    /* Nim */

    /* Squirrel */
    {".nut", PDXE_LANG_SQUIRREL},

    /* Odin */
    {".odin", PDXE_LANG_ODIN},

    /* DeviceTree */
    {".overlay", PDXE_LANG_DEVICETREE},

    /* Pascal */
    {".pas", PDXE_LANG_PASCAL},

    /* Diff */
    {".patch", PDXE_LANG_DIFF},

    /* Pine Script */
    {".pine", PDXE_LANG_PINE},

    /* Pkl */
    {".pkl", PDXE_LANG_PKL},

    /* PO */
    {".po", PDXE_LANG_PO},

    /* Pony */
    {".pony", PDXE_LANG_PONY},

    /* PO */
    {".pot", PDXE_LANG_PO},

    /* Puppet */
    {".pp", PDXE_LANG_PUPPET},

    /* Prisma */
    {".prisma", PDXE_LANG_PRISMA},

    /* Properties */
    {".properties", PDXE_LANG_PROPERTIES},

    /* PowerShell */
    {".ps1", PDXE_LANG_POWERSHELL},

    /* PowerShell */
    {".psd1", PDXE_LANG_POWERSHELL},

    /* PowerShell */
    {".psm1", PDXE_LANG_POWERSHELL},

    /* PureScript */
    {".purs", PDXE_LANG_PURESCRIPT},

    /* ReScript */
    {".res", PDXE_LANG_RESCRIPT},

    /* ReScript */
    {".resi", PDXE_LANG_RESCRIPT},

    /* Regex */
    {".re", PDXE_LANG_REGEX},

    /* Racket */
    {".rkt", PDXE_LANG_RACKET},

    /* RON */
    {".ron", PDXE_LANG_RON},

    /* reStructuredText */
    {".rst", PDXE_LANG_RST},

    /* Assembly */
    {".s", PDXE_LANG_ASSEMBLY},

    /* Assembly */
    {".S", PDXE_LANG_ASSEMBLY},

    /* Scheme */
    {".scm", PDXE_LANG_SCHEME},

    /* Chialisp — .clsp puzzles, .clib/.clinc includable libraries */
    {".clsp", PDXE_LANG_CHIALISP},
    {".clib", PDXE_LANG_CHIALISP},
    {".clinc", PDXE_LANG_CHIALISP},

    /* Slang */
    {".slang", PDXE_LANG_SLANG},

    /* Smali */
    {".smali", PDXE_LANG_SMALI},

    /* Smithy */
    {".smithy", PDXE_LANG_SMITHY},

    /* Solidity */
    {".sol", PDXE_LANG_SOLIDITY},

    /* SOQL */
    {".soql", PDXE_LANG_SOQL},

    /* SOSL */
    {".sosl", PDXE_LANG_SOSL},

    /* Scheme */
    {".ss", PDXE_LANG_SCHEME},

    /* Starlark */
    {".star", PDXE_LANG_STARLARK},

    /* SystemVerilog */

    /* SystemVerilog */

    /* Sway */
    {".sw", PDXE_LANG_SWAY},

    /* Tcl */
    {".tcl", PDXE_LANG_TCL},

    /* TableGen */
    {".td", PDXE_LANG_TABLEGEN},

    /* Templ */
    {".templ", PDXE_LANG_TEMPL},

    /* Thrift */
    {".thrift", PDXE_LANG_THRIFT},

    /* Teal */
    {".tl", PDXE_LANG_TEAL},

    /* TLA+ */
    {".tla", PDXE_LANG_TLAPLUS},

    /* Go Template */
    {".tmpl", PDXE_LANG_GOTEMPLATE},

    /* Apex */
    {".trigger", PDXE_LANG_APEX},

    /* Typst */
    {".typ", PDXE_LANG_TYPST},

    /* VHDL */
    {".vhd", PDXE_LANG_VHDL},

    /* VHDL */
    {".vhdl", PDXE_LANG_VHDL},

    /* WGSL */
    {".wgsl", PDXE_LANG_WGSL},

    /* WIT */
    {".wit", PDXE_LANG_WIT},

    /* Zsh */
    {".zsh", PDXE_LANG_ZSH},

    /* Zig */
    {".zig", PDXE_LANG_ZIG},
};

typedef struct {
    const char *filename;
    PDXELanguage language;
} filename_entry_t;

static const filename_entry_t FILENAME_TABLE[] = {
    {"CMakeLists.txt", PDXE_LANG_CMAKE},
    {"Dockerfile", PDXE_LANG_DOCKERFILE},
    {"GNUmakefile", PDXE_LANG_MAKEFILE},
    {"Makefile", PDXE_LANG_MAKEFILE},
    {"makefile", PDXE_LANG_MAKEFILE},
    {"meson.build", PDXE_LANG_MESON},
    {"meson.options", PDXE_LANG_MESON},
    {"meson_options.txt", PDXE_LANG_MESON},
    {"kustomization.yaml", PDXE_LANG_KUSTOMIZE},
    {"kustomization.yml", PDXE_LANG_KUSTOMIZE},
    /* Note: FILENAME_TABLE uses case-sensitive strcmp, so mixed-case variants
     * (e.g. "Kustomization.yaml") are not matched here.  They fall through to
     * PDXE_LANG_YAML and are re-classified by pdxe_is_kustomize_file() in
     * pass_k8s.c, which performs a case-insensitive comparison.  This is the
     * intended behaviour — no additional entries are needed. */
    {".vimrc", PDXE_LANG_VIMSCRIPT},
    {".zshrc", PDXE_LANG_ZSH},
    {".zshenv", PDXE_LANG_ZSH},
    {".zprofile", PDXE_LANG_ZSH},
    {"justfile", PDXE_LANG_JUST},
    {"Justfile", PDXE_LANG_JUST},
    {".justfile", PDXE_LANG_JUST},
    {"hyprland.conf", PDXE_LANG_HYPRLANG},
    {"ssh_config", PDXE_LANG_SSHCONFIG},
    {"sshd_config", PDXE_LANG_SSHCONFIG},
    {".ssh/config", PDXE_LANG_SSHCONFIG},
    {"BUILD", PDXE_LANG_STARLARK},
    {"BUILD.bazel", PDXE_LANG_STARLARK},
    {"WORKSPACE", PDXE_LANG_STARLARK},
    {"WORKSPACE.bazel", PDXE_LANG_STARLARK},
    {"requirements.txt", PDXE_LANG_REQUIREMENTS},
    {"requirements-dev.txt", PDXE_LANG_REQUIREMENTS},
    {"requirements-test.txt", PDXE_LANG_REQUIREMENTS},
    {"Kconfig", PDXE_LANG_KCONFIG},
    {"go.mod", PDXE_LANG_GOMOD},
    {".env", PDXE_LANG_DOTENV},
    {".env.local", PDXE_LANG_DOTENV},
    {".gitattributes", PDXE_LANG_GITATTRIBUTES},

};

PDXELanguage pdxe_language_for_extension(const char *ext) {
    if (!ext || !ext[0]) {
        return PDXE_LANG_COUNT;
    }

    /* Check user-defined overrides first */
    const pdxe_userconfig_t *ucfg = pdxe_get_user_lang_config();
    if (ucfg) {
        PDXELanguage ulang = pdxe_userconfig_lookup(ucfg, ext);
        if (ulang != PDXE_LANG_COUNT) {
            return ulang;
        }
    }

    for (size_t i = 0; i < EXT_TABLE_SIZE; i++) {
        if (strcmp(EXT_TABLE[i].ext, ext) == 0) {
            return EXT_TABLE[i].language;
        }
    }
    return PDXE_LANG_COUNT;
}

PDXELanguage pdxe_language_for_filename(const char *filename) {
    if (!filename || !filename[0]) {
        return PDXE_LANG_COUNT;
    }

    /* Check special filenames first */
    for (size_t i = 0; i < FILENAME_TABLE_SIZE; i++) {
        if (strcmp(FILENAME_TABLE[i].filename, filename) == 0) {
            return FILENAME_TABLE[i].language;
        }
    }

    /* DotEnv variant filenames (".env.local", ".env.production", …): the
     * filename starts with ".env." but its last "extension" (e.g. ".local")
     * is not a real language extension.  Match the dotenv convention used by
     * pass_envscan/pass_infrascan (".env" exact, ".env." prefix, "*.env"
     * suffix) so file-index routing agrees with direct extraction. */
    if (strncmp(filename, ".env.", SLEN(".env.")) == 0) {
        return PDXE_LANG_DOTENV;
    }

    /* Fall back to extension-based lookup.
     * For compound extensions (e.g. ".blade.php") defined in the user config,
     * scan from the first dot in the basename toward the last, checking user
     * config at each position.  Built-in extensions use the last dot only. */
    const char *last_dot = strrchr(filename, '.');
    if (!last_dot) {
        return PDXE_LANG_COUNT;
    }

    /* Probe compound extensions (e.g. ".blade.php") from the first dot toward
     * the last. Built-in compounds are checked first so e.g. Laravel Blade
     * templates map to Blade rather than the single-extension fallback (PHP);
     * user config can still add more (#258). */
    static const struct {
        const char *ext;
        PDXELanguage lang;
    } COMPOUND_EXT_TABLE[] = {
        {".blade.php", PDXE_LANG_BLADE},
    };
    const pdxe_userconfig_t *ucfg = pdxe_get_user_lang_config();
    const char *p = strchr(filename, '.');
    while (p && p < last_dot) {
        for (size_t i = 0; i < sizeof(COMPOUND_EXT_TABLE) / sizeof(COMPOUND_EXT_TABLE[0]); i++) {
            if (strcmp(p, COMPOUND_EXT_TABLE[i].ext) == 0) {
                return COMPOUND_EXT_TABLE[i].lang;
            }
        }
        if (ucfg) {
            PDXELanguage lang = pdxe_userconfig_lookup(ucfg, p);
            if (lang != PDXE_LANG_COUNT) {
                return lang;
            }
        }
        p = strchr(p + SKIP_ONE, '.');
    }

    /* Standard single-extension lookup (built-ins + user overrides). */
    return pdxe_language_for_extension(last_dot);
}
