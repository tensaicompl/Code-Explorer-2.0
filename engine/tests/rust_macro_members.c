/*
 * The members a Rust `macro_rules!` invocation may write (docs/plan/ISSUES.md, issue 110;
 * engine/patches/0030-rust-macro-members.patch).
 *
 *     rust_macro_members
 *
 * An item a macro invocation writes is no definition: extraction reads the file's
 * syntax, and an expansion is in no file. A type whose members a macro writes in part
 * had only its handwritten ones in the registry, and Stage 3 drew a call to the one
 * member of the name it held. The extraction now records, for every invocation of a
 * definition it sees in its own file, the type and member each `impl` of the definition's
 * transcribers writes, read from the definition's tokens, never by expanding it:
 *
 *   - a literal type, a type or member the transcriber names by a metavariable (what the
 *     engine's matcher binds it to, or with no rule matched each identifier of the
 *     invocation's arguments), a generated member name (any member), an invocation in
 *     an impl body (that impl's type), and a repetition's impls;
 *   - none for a macro no invocation sees, a function nested in a method's body, or a
 *     file of another language;
 *   - one entry per type and member, in byte order, across the interface.
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pdxe.h"
#include "pdxe_core.h"
#include "shim_internal.h"

static int failures = 0;

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL ");                                                                       \
            printf(__VA_ARGS__);                                                                   \
            printf("\n");                                                                          \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static pdxe_ctx *ctx = NULL;

/* The members a file's invocations may write, as the interface gives them, joined as
 * "Type.member" (".*" for any member) with ";" between, in their order. */
static void members_of(const char *lang, const char *rel, const char *source, char *out,
                       size_t cap) {
    pdxe_file_result *r = NULL;
    out[0] = '\0';
    if (pdxe_extract_file(ctx, pdxe_language_id(lang), rel, (const uint8_t *)source,
                          strlen(source), &r) != PDXE_OK ||
        !r) {
        printf("FAIL %s: the extraction failed\n", rel);
        exit(1);
    }
    const PDXEFileResult *e = pdxe_result_engine(r);
    CHECK((uint32_t)e->macro_members.count == r->n_macro_members,
          "%s: the interface gives %u members, the engine %d", rel, r->n_macro_members,
          e->macro_members.count);
    for (uint32_t i = 0; i < r->n_macro_members; i++) {
        const pdxe_macro_member *m = &r->macro_members[i];
        CHECK(m->type_name == e->macro_members.items[i].type_name &&
                  m->member == e->macro_members.items[i].member,
              "%s: member %u is not the engine's", rel, i);
        size_t used = strlen(out);
        snprintf(out + used, cap - used, "%s%s.%s", i ? ";" : "", m->type_name,
                 m->member ? m->member : "*");
    }
    pdxe_result_free(ctx, r);
}

static void expect(const char *what, const char *source, const char *want) {
    char got[512];
    members_of("rust", "src/lib.rs", source, got, sizeof got);
    CHECK(strcmp(got, want) == 0, "%s: members \"%s\", want \"%s\"", what, got, want);
}

/* The issue's shape: a handwritten `From<()>` impl beside the ones a macro writes. */
static void check_literal_type(void) {
    expect("literal type",
           "pub struct Payload(String);\n"
           "impl From<()> for Payload {\n"
           "    fn from(_: ()) -> Self { Payload(String::new()) }\n"
           "}\n"
           "macro_rules! make_impl {\n"
           "    ($ty:ty) => {\n"
           "        impl From<$ty> for Payload {\n"
           "            fn from(v: $ty) -> Self { Payload(v.to_string()) }\n"
           "        }\n"
           "    };\n"
           "}\n"
           "make_impl!(u32);\n"
           "make_impl!(&'static str);\n",
           "Payload.from");
    expect("a path to the type, generics, where",
           "macro_rules! conv {\n"
           "    () => {\n"
           "        impl<T> Into2<T> for crate::gen::Gen<T> where T: Clone {\n"
           "            fn conv(&self) {}\n"
           "            type Hook = fn(u8) -> u8;\n"
           "            pub(crate) const fn other() -> u8 { 0 }\n"
           "        }\n"
           "        impl<'a> Lens for &'a mut Gen<u8> { fn focus(&self) {} }\n"
           "    };\n"
           "}\n"
           "conv!();\n",
           "Gen.conv;Gen.focus;Gen.other");
}

static void check_metavariables(void) {
    expect("a type named by a metavariable",
           "macro_rules! via {\n"
           "    ($t:ident) => {\n"
           "        impl From<u8> for $t { fn from(_: u8) -> Self { $t::default() } }\n"
           "    };\n"
           "}\n"
           "via!(Gadget);\n",
           "Gadget.from");
    expect("a member named by a metavariable",
           "pub struct Widget;\n"
           "macro_rules! named {\n"
           "    ($n:ident) => { impl Widget { pub fn $n() -> u8 { 0 } } };\n"
           "}\n"
           "named!(alpha);\n",
           "Widget.alpha");
    expect("the bound type, never the arguments' other identifiers",
           "macro_rules! rej {\n"
           "    (pub struct $name:ident ($inner:ident);) => {\n"
           "        impl Show for $name { fn show(&self) {} }\n"
           "    };\n"
           "}\n"
           "rej! { pub struct Wrapper(Inner); }\n",
           "Wrapper.show");
    expect("only the rules the invocation matches",
           "macro_rules! two {\n"
           "    (a $t:ident) => { impl $t { fn one() {} } };\n"
           "    (b $t:ident) => { impl $t { fn two() {} } };\n"
           "}\n"
           "two!(a Thing);\n",
           "Thing.one");
    expect("a bound type's path, its last name",
           "macro_rules! wrap {\n"
           "    ($t:ty) => { impl From<u8> for $t { fn from(_: u8) -> Self { todo!() } } };\n"
           "}\n"
           "wrap!(crate::w::Wrapped<u8>);\n"
           "wrap!(&'static [u8]);\n",
           "Wrapped.from");
    expect("no rule matched: each identifier of the arguments",
           "macro_rules! odd {\n"
           "    (struct $t:ident) => { impl $t { fn $t() {} } };\n"
           "}\n"
           "odd!(Alpha, Beta);\n",
           "Alpha.Alpha;Alpha.Beta;Beta.Alpha;Beta.Beta");
    expect("a generated member name",
           "macro_rules! made {\n"
           "    ($t:ident) => { impl $t { fn [<get_ $t>]() {} } };\n"
           "}\n"
           "made!(Thing);\n",
           "Thing.*");
    expect("an impl nested in the transcriber",
           "macro_rules! hidden {\n"
           "    () => { const _: () = { impl Hidden { pub fn inner() {} } }; };\n"
           "}\n"
           "hidden!();\n",
           "Hidden.inner");
    expect("a repetition's impls",
           "macro_rules! many {\n"
           "    ($($t:ty),*) => { $( impl From<$t> for Many { fn from(_: $t) -> Self { Many } } )* };\n"
           "}\n"
           "many!(u8, u16);\n",
           "Many.from");
}

static void check_impl_body(void) {
    expect("an invocation in an impl body",
           "pub struct Thing;\n"
           "macro_rules! getter { () => { pub fn get(&self) -> u8 { 1 } }; }\n"
           "impl Thing {\n"
           "    getter!();\n"
           "    pub fn put(&self) {}\n"
           "}\n",
           "Thing.get");
}

static void check_none(void) {
    expect("a macro no invocation sees",
           "pub struct Payload;\n"
           "early!();\n"
           "macro_rules! early { () => { impl From<i8> for Payload { fn from(_: i8) -> Self { Payload } } }; }\n"
           "macro_rules! unused { () => { impl From<i16> for Payload { fn from(_: i16) -> Self { Payload } } }; }\n",
           "");
    expect("a function nested in a method's body",
           "macro_rules! outer {\n"
           "    () => { impl From<u8> for Outer { fn from(_: u8) -> Self { fn helper() {} Outer } } };\n"
           "}\n"
           "outer!();\n",
           "Outer.from");
    expect("a macro outside the file",
           "external_macro!(Payload);\n"
           "crate::other_macro! { impl From<u8> for Payload { fn from(_: u8) -> Self { Payload } } }\n",
           "");
    char got[512];
    members_of("python", "m.py", "def f():\n    return 1\n", got, sizeof got);
    CHECK(strcmp(got, "") == 0, "python: members \"%s\"", got);
}

static void check_order(void) {
    expect("one entry per type and member, in byte order",
           "macro_rules! zz {\n"
           "    ($t:ident) => {\n"
           "        impl B for $t { fn b(&self) {} fn a(&self) {} }\n"
           "        impl A for Zed { fn z(&self) {} }\n"
           "    };\n"
           "}\n"
           "zz!(Alpha);\n"
           "zz!(Alpha);\n"
           "mod inner {\n"
           "    zz!(Beta);\n"
           "}\n",
           "Alpha.a;Alpha.b;Beta.a;Beta.b;Zed.z");
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_literal_type();
    check_metavariables();
    check_impl_body();
    check_none();
    check_order();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok: the members a Rust macro invocation may write are recorded\n");
    return 0;
}
