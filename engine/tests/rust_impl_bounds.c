/*
 * A Rust trait impl applies only when its header holds (docs/plan/ISSUES.md, issue 109;
 * engine/patches/0032-rust-impl-applicability.patch).
 *
 *     rust_impl_bounds
 *
 * `Type::method` reaches a trait's method through an impl of the trait for the type,
 * and an impl applies to a type only when what its header requires holds for it. The
 * typed pass took the type's one impl with a method of the name, whatever its header
 * required: `WithRejection::<TestExtractor, TestRejection>::from_request` was answered
 * with `WithRejection`'s own `FromRequest` impl, whose `E: FromRequest<S>` the type
 * argument does not meet. Extraction now records each trait impl's header, and the typed
 * pass answers an impl's method only when every predicate is proven to hold:
 *
 *   - the header as extraction writes it: the self type's form (a path of distinct
 *     parameters, a parameter, anything else), the parameters, the self type's and the
 *     trait's arguments, every predicate's bounds (trait, relaxed, lifetime, other),
 *     comments and whitespace left out; an impl's methods carry its relation's header,
 *     an inherent impl none;
 *   - T1 an impl with no requirement applies; T2 a bound proven from the site's type
 *     argument and an unconditional impl of the trait for it; T3 a blanket impl whose
 *     one bound is relaxed applies, one whose bound needs proof does not; T4 a bound no
 *     impl satisfies, T5 a bound on a parameter the path does not give, on a projection,
 *     a higher-ranked bound, a bound with no type argument given, and an impl of the
 *     bound's trait that is itself conditional are never proven; T6 and T7, two impls of
 *     the name for one type: answered only when one is proven and no other is an impl of
 *     its trait, never when an inherent method of the name is beside them; a trait's
 *     default method reached through the type's impl of the trait, under the same proof;
 *   - the issue's shape, both of its calls, and the shape without the bound on `S`;
 *   - the engine differential's site: `T::from_request` on an impl's parameter is not
 *     the blanket impl's by that parameter's name;
 *   - a `#[derive]`'s synthesised method is answered as it was.
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

typedef struct {
    pdxe_file_result *r;
    const PDXEFileResult *e;
} extraction_t;

static extraction_t extract(const char *rel, const char *source) {
    extraction_t x = {0};
    if (pdxe_extract_file(ctx, pdxe_language_id("rust"), rel, (const uint8_t *)source,
                          strlen(source), &x.r) != PDXE_OK ||
        !x.r) {
        printf("FAIL %s: the extraction failed\n", rel);
        exit(1);
    }
    x.e = pdxe_result_engine(x.r);
    return x;
}

static bool ends_with(const char *s, const char *suffix) {
    size_t n = s ? strlen(s) : 0, m = strlen(suffix);
    return n >= m && strcmp(s + n - m, suffix) == 0;
}

static void joined(const char *const *entries, char *out, size_t cap) {
    out[0] = '\0';
    if (!entries) {
        snprintf(out, cap, "(none)");
        return;
    }
    for (int i = 0; entries[i]; i++) {
        size_t used = strlen(out);
        snprintf(out + used, cap - used, "%s%s", i ? "|" : "", entries[i]);
    }
}

/* The header of the impl of `trait` for the type spelled `type`, joined with "|". */
static void header_of(const PDXEFileResult *e, const char *trait, const char *type, char *out,
                      size_t cap) {
    snprintf(out, cap, "(no relation)");
    for (int i = 0; i < e->impl_traits.count; i++) {
        const PDXEImplTrait *it = &e->impl_traits.items[i];
        if (strcmp(it->trait_name, trait) == 0 && strcmp(it->struct_name, type) == 0) {
            joined(it->impl_bounds, out, cap);
            return;
        }
    }
}

static void expect_header(const char *what, const char *source, const char *trait,
                          const char *type, const char *want) {
    extraction_t x = extract("src/lib.rs", source);
    char got[1024];
    header_of(x.e, trait, type, got, sizeof got);
    CHECK(strcmp(got, want) == 0, "%s: header \"%s\", want \"%s\"", what, got, want);
    pdxe_result_free(ctx, x.r);
}

static void check_headers(void) {
    expect_header("no generics", "trait Tr { fn f(); }\nstruct A;\nimpl Tr for A { fn f() {} }\n",
                  "Tr", "A", "s:plain");
    expect_header("a parameter, unbounded",
                  "trait Tr {}\nstruct W<T>(T);\nimpl<T> Tr for W<T> {}\n", "Tr", "W",
                  "s:plain|p:T|a:T");
    expect_header("inline, relaxed and where bounds",
                  "trait Tr {}\nstruct W<T: ?Sized>(Box<T>);\n"
                  "impl<T: Clone + ?Sized> Tr for W<T> where T: std::fmt::Debug {}\n",
                  "Tr", "W", "s:plain|p:T|a:T|w:T|b:Clone|r:?Sized|w:T|b:std::fmt::Debug");
    expect_header("the issue's impl",
                  "impl<E, R, S> FromRequest<S> for WithRejection<E, R>\nwhere\n"
                  "    S: Send + Sync,\n    E: FromRequest<S>,\n"
                  "    R: From<E::Rejection> + IntoResponse,\n{\n}\n",
                  "FromRequest", "WithRejection",
                  "s:plain|p:E|p:R|p:S|a:E|a:R|t:S|w:S|b:Send|b:Sync|w:E|b:FromRequest|g:S|w:R|"
                  "b:From|g:E::Rejection|b:IntoResponse");
    expect_header("a blanket impl",
                  "impl<S, T> FromRequest<S, private::ViaParts> for T\nwhere\n"
                  "    S: Send + Sync,\n    T: FromRequestParts<S>,\n{\n}\n",
                  "FromRequest", "T",
                  "s:blanket|p:S|p:T|t:S|t:private::ViaParts|w:S|b:Send|b:Sync|w:T|"
                  "b:FromRequestParts|g:S");
    expect_header("a concrete argument", "trait Tr {}\nstruct W<T>(T);\nimpl Tr for W<u8> {}\n",
                  "Tr", "W", "s:other");
    expect_header("a parameter twice",
                  "trait Tr {}\nstruct W<A, B>(A, B);\nimpl<T> Tr for W<T, T> {}\n", "Tr", "W",
                  "s:other|p:T");
    expect_header("lifetimes, an associated binding and comments",
                  "trait Tr<'a> {}\nstruct W<'a, I>(&'a I);\n"
                  "impl<'a, I: 'a> Tr<'a> for W<'a, I>\nwhere\n"
                  "    I: Iterator< /* the items */ Item = u8 >, // bytes\n{\n}\n",
                  "Tr", "W", "s:plain|p:I|a:I|w:I|l:'a|w:I|b:Iterator|g:Item=u8");
    expect_header("a const parameter and a dyn type argument",
                  "trait Tr<T: ?Sized> {}\nstruct W<const N: usize>;\n"
                  "impl<const N: usize> Tr<dyn std::error::Error> for W<N> {}\n",
                  "Tr", "W", "s:plain|p:N|a:N|t:dyn std::error::Error");

    /* A higher-ranked bound is another bound, never read as a trait. */
    {
        extraction_t x = extract("src/lib.rs", "trait Tr {}\nstruct W<F>(F);\n"
                                               "impl<F> Tr for W<F> where F: for<'a> Fn(&'a u8) {}\n");
        char got[256];
        header_of(x.e, "Tr", "W", got, sizeof got);
        const char *want = "s:plain|p:F|a:F|w:F|o:for<";
        CHECK(strncmp(got, want, strlen(want)) == 0, "a higher-ranked bound: \"%s\"", got);
        pdxe_result_free(ctx, x.r);
    }

    /* The impl's methods carry the relation's header; an inherent impl's carry none. */
    {
        extraction_t x = extract("src/lib.rs",
                                 "trait Tr { fn f(); }\nstruct W<T>(T);\n"
                                 "impl<T: Copy> Tr for W<T> { fn f() {} }\n"
                                 "impl<T> W<T> { fn g() {} }\n");
        const char *const *relation = NULL;
        for (int i = 0; i < x.e->impl_traits.count; i++) {
            relation = x.e->impl_traits.items[i].impl_bounds;
        }
        int methods = 0;
        for (int i = 0; i < x.e->defs.count; i++) {
            const PDXEDefinition *d = &x.e->defs.items[i];
            if (ends_with(d->qualified_name, ".W.f")) {
                methods++;
                CHECK(d->impl_bounds == relation, "the trait impl's method has not its header");
            } else if (ends_with(d->qualified_name, ".W.g")) {
                methods++;
                CHECK(d->impl_bounds == NULL, "an inherent method has a header");
            } else {
                CHECK(d->impl_bounds == NULL, "%s has a header", d->qualified_name);
            }
        }
        CHECK(methods == 2 && relation, "the methods or the relation are missing");
        pdxe_result_free(ctx, x.r);
    }

    /* Through the surface, the header is the same. */
    {
        extraction_t x = extract("src/lib.rs",
                                 "trait Tr { fn f(); }\nstruct W<T>(T);\n"
                                 "impl<T: Copy> Tr for W<T> { fn f() {} }\n");
        uint8_t *bytes = NULL;
        size_t len = 0;
        CHECK(pdxe_surface_export(x.r, "src/lib.rs", &bytes, &len) == PDXE_OK, "no surface");
        PDXEFileResult *decoded = NULL;
        char *path = NULL;
        int lang = 0;
        uint32_t lost = 0;
        pdxe_surface_texts *texts = NULL;
        CHECK(bytes && pdxe_surface_decode(bytes, len, &decoded, &path, &lang, &lost, &texts) ==
                           PDXE_OK,
              "the surface does not decode");
        if (decoded) {
            char want[256], got[256], method[256] = "(none)";
            joined(x.e->impl_traits.items[0].impl_bounds, want, sizeof want);
            joined(decoded->impl_traits.items[0].impl_bounds, got, sizeof got);
            for (int i = 0; i < decoded->defs.count; i++) {
                if (strcmp(decoded->defs.items[i].name, "f") == 0) {
                    joined(decoded->defs.items[i].impl_bounds, method, sizeof method);
                }
            }
            CHECK(strcmp(want, got) == 0, "the surface's relation header \"%s\", want \"%s\"",
                  got, want);
            CHECK(strcmp(want, method) == 0, "the surface's method header \"%s\", want \"%s\"",
                  method, want);
            pdxe_free_result(decoded);
        }
        free(path);
        pdxe_surface_texts_free(texts);
        pdxe_surface_free(bytes);
        pdxe_result_free(ctx, x.r);
    }
}

/* The per-file typed answers from `caller` whose target ends with `target`; `strategy`
 * the last one's. */
static int answers(const PDXEFileResult *e, const char *caller, const char *target,
                   const char **strategy) {
    char suffix[64];
    snprintf(suffix, sizeof suffix, ".%s", caller);
    int n = 0;
    for (int i = 0; i < e->resolved_calls.count; i++) {
        const PDXEResolvedCall *c = &e->resolved_calls.items[i];
        if (!c->reason && ends_with(c->caller_qn, suffix) && ends_with(c->callee_qn, target)) {
            n++;
            if (strategy) {
                *strategy = c->strategy;
            }
        }
    }
    return n;
}

/* Whether `caller`'s call is answered with `target`, a method of an impl, by the
 * `Type::method` route. */
static void expect_answer(const char *what, const char *source, const char *caller,
                          const char *target, bool want) {
    extraction_t x = extract("src/lib.rs", source);
    const char *strategy = NULL;
    int n = answers(x.e, caller, target, &strategy);
    if (want) {
        CHECK(n == 1 && strategy &&
                  (strcmp(strategy, "lsp_ufcs") == 0 || strcmp(strategy, "lsp_trait_ufcs") == 0),
              "%s: `%s` is not answered with %s (%d answers, %s)", what, caller, target, n,
              strategy ? strategy : "none");
    } else {
        CHECK(n == 0, "%s: `%s` is answered with %s (%s)", what, caller, target,
              strategy ? strategy : "?");
    }
    pdxe_result_free(ctx, x.r);
}

#define SHOW_MAKE                                                                                  \
    "trait Show {}\ntrait Make { fn make(); }\npub struct A;\npub struct B;\n"                       \
    "impl Show for A {}\npub struct W<X>(X);\n"

static void check_answers(void) {
    /* T1: an impl with no requirement applies to every instance of its type. */
    expect_answer("T1", "trait T { fn f(); }\npub struct A;\nimpl T for A { fn f() {} }\n"
                        "pub fn run() { A::f(); }\n",
                  "run", ".A.f", true);
    expect_answer("T1, a parameter without bounds",
                  "trait T { fn f(); }\npub struct W<X>(X);\nimpl<X> T for W<X> { fn f() {} }\n"
                  "pub fn run() { W::<u8>::f(); }\n",
                  "run", ".W.f", true);
    expect_answer("T1, a parameter without bounds, no argument given",
                  "trait T { fn f(); }\npub struct W<X>(X);\nimpl<X> T for W<X> { fn f() {} }\n"
                  "pub fn run() { W::f(); }\n",
                  "run", ".W.f", true);

    /* T2: the bound holds for the type the path gives, by an unconditional impl. */
    expect_answer("T2", SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                                  "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T2, from the where clause",
                  SHOW_MAKE "impl<X> Make for W<X> where X: Show { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T2, a relaxed and a lifetime bound beside it",
                  SHOW_MAKE "impl<X: ?Sized + Show + 'static> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);

    /* T3: a blanket impl applies to every type only when its bounds need no proof. */
    expect_answer("T3, a relaxed bound",
                  "trait Describe { fn describe(); }\nimpl<T: ?Sized> Describe for T { fn describe() {} }\n"
                  "pub fn run<T>() { T::describe(); }\n",
                  "run", ".T.describe", true);
    expect_answer("T3, a relaxed and a lifetime bound",
                  "trait Describe { fn describe(); }\n"
                  "impl<T: ?Sized + 'static> Describe for T { fn describe() {} }\n"
                  "pub fn run<T>() { T::describe(); }\n",
                  "run", ".T.describe", true);
    expect_answer("T3, a bound to prove",
                  "trait Show {}\ntrait Describe { fn describe(); }\n"
                  "impl<T: Show> Describe for T { fn describe() {} }\n"
                  "pub fn run<T: Show>() { T::describe(); }\n",
                  "run", ".T.describe", false);

    /* The bound's trait arguments: an impl of the trait with other arguments proves
     * nothing; one with the same, or with a parameter of its own there, does. */
    expect_answer("T2, the bound's trait arguments",
                  "trait Conv<Q> {}\ntrait Make { fn make(); }\npub struct A;\npub struct W<X>(X);\n"
                  "impl Conv<u8> for A {}\nimpl<X: Conv<u8>> Make for W<X> { fn make() {} }\n"
                  "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T2, an impl free in the trait's argument",
                  "trait Conv<Q> {}\ntrait Make { fn make(); }\npub struct A;\npub struct W<X>(X);\n"
                  "impl<Q> Conv<Q> for A {}\nimpl<X: Conv<u8>> Make for W<X> { fn make() {} }\n"
                  "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T4, an impl of the trait with other arguments",
                  "trait Conv<Q> {}\ntrait Make { fn make(); }\npub struct A;\npub struct W<X>(X);\n"
                  "impl Conv<u16> for A {}\nimpl<X: Conv<u8>> Make for W<X> { fn make() {} }\n"
                  "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);

    /* A trait's default method, reached through the type's one impl of the trait: the
     * same proof, of that impl's header. */
    expect_answer("T2, a trait's default through the type's impl",
                  "trait Show {}\ntrait Greet { fn greet() {} }\npub struct A;\npub struct B;\n"
                  "impl Show for A {}\npub struct W<X>(X);\nimpl<X: Show> Greet for W<X> {}\n"
                  "pub fn run() { W::<A>::greet(); }\n",
                  "run", ".Greet.greet", true);
    expect_answer("T4, a trait's default through an impl not proven",
                  "trait Show {}\ntrait Greet { fn greet() {} }\npub struct A;\npub struct B;\n"
                  "impl Show for A {}\npub struct W<X>(X);\nimpl<X: Show> Greet for W<X> {}\n"
                  "pub fn run() { W::<B>::greet(); }\n",
                  "run", ".Greet.greet", false);

    /* T4: no impl of the bound's trait for the type: never answered. */
    expect_answer("T4", SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                                  "pub fn run() { W::<B>::make(); }\n",
                  "run", ".W.make", false);

    /* T5: what is unknown is never proven. */
    expect_answer("T5, no type argument given",
                  SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T5, a parameter the path does not give",
                  "trait Make<S> { fn make(); }\npub struct A;\n"
                  "impl<S> Make<S> for A where S: Send { fn make() {} }\n"
                  "pub fn run() { A::make(); }\n",
                  "run", ".A.make", false);
    expect_answer("T5, a projection",
                  SHOW_MAKE "impl<X: Iterator> Make for W<X> where X::Item: Show { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T5, a higher-ranked bound",
                  "trait Make { fn make(); }\npub struct W<F>(F);\npub struct A;\n"
                  "impl<F> Make for W<F> where F: for<'a> Fn(&'a u8) { fn make() {} }\n"
                  "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T5, the bound's own impl is conditional",
                  "trait Show {}\ntrait Make { fn make(); }\npub struct A<Y>(Y);\npub struct W<X>(X);\n"
                  "impl<Y: Copy> Show for A<Y> {}\nimpl<X: Show> Make for W<X> { fn make() {} }\n"
                  "pub fn run() { W::<A<u8>>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T5, a type parameter of the caller named like a type",
                  SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run<A>() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T5, an inferred argument",
                  SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<_>::make(); }\n",
                  "run", ".W.make", false);

    /* T6 and T7: two impls of the name for one type. The one proven is the target only
     * when it is the only one proven; one not proven may be the one that applies. */
    expect_answer("T6, the other unproven (an argument it does not have)",
                  SHOW_MAKE "trait Make2 { fn make(); }\n"
                            "impl Make2 for W<u8> { fn make() {} }\n"
                            "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T6, the other unproven (a bound unknown)",
                  SHOW_MAKE "trait Make2 { fn make(); }\n"
                            "impl<X: Copy> Make2 for W<X> { fn make() {} }\n"
                            "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", true);
    expect_answer("T7, neither proven",
                  SHOW_MAKE "trait Make2 { fn make(); }\n"
                            "impl<X: Copy> Make for W<X> { fn make() {} }\n"
                            "impl<X: Clone> Make2 for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T7, both proven",
                  SHOW_MAKE "trait Make2 { fn make(); }\n"
                            "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "impl<X> Make2 for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T7, two impls of one trait",
                  "pub struct Wide(u32);\nimpl From<u8> for Wide { fn from(n: u8) -> Self { Wide(n as u32) } }\n"
                  "impl From<u16> for Wide { fn from(n: u16) -> Self { Wide(n as u32) } }\n"
                  "pub fn run() -> Wide { Wide::from(1u8) }\n",
                  "run", ".Wide.from", false);
    expect_answer("T7, one impl of the trait proven, another of it unproven",
                  SHOW_MAKE "trait Pick<Q> { fn make(); }\n"
                            "impl<X: Show> Pick<u8> for W<X> { fn make() {} }\n"
                            "impl Pick<u16> for W<u32> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T7, an inherent method of the name beside",
                  SHOW_MAKE "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "impl<X: Copy> W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);
    expect_answer("T7, an inherent method of the name beside, first",
                  SHOW_MAKE "impl<X: Copy> W<X> { fn make() {} }\n"
                            "impl<X: Show> Make for W<X> { fn make() {} }\n"
                            "pub fn run() { W::<A>::make(); }\n",
                  "run", ".W.make", false);

    /* An inherent method is the type's own associated item: answered as before. */
    expect_answer("an inherent method under bounds",
                  "pub struct W<X>(X);\nimpl<X: Clone> W<X> { pub fn make() {} }\n"
                  "pub fn run() { W::<u8>::make(); }\n",
                  "run", ".W.make", true);
    /* A derive's synthesised method names no definition: answered as before. */
    {
        extraction_t x = extract("src/lib.rs", "#[derive(Default)]\npub struct W<X>(X);\n"
                                               "pub fn run() { W::<u8>::default(); }\n");
        CHECK(answers(x.e, "run", ".W.default", NULL) == 1,
              "a derive's method is no longer answered");
        pdxe_result_free(ctx, x.r);
    }
}

/* The issue: `WithRejection<E, R>` implements `FromRequest<S>` when `E: FromRequest<S>`
 * and `FromRequestParts<S>` when `E: FromRequestParts<S>`; `TestExtractor` implements
 * only `FromRequestParts`, so `from_request` reaches the blanket impl through it, and
 * `WithRejection`'s own `from_request` is not the call's target. */
#define ISSUE_SHAPE(bound)                                                                         \
    "pub trait FromRequestParts<S> { fn from_request_parts(state: &S); }\n"                        \
    "pub trait FromRequest<S, M = ViaRequest> { fn from_request(state: &S); }\n"                   \
    "pub enum ViaRequest {}\npub enum ViaParts {}\n"                                               \
    "impl<S, T> FromRequest<S, ViaParts> for T where T: FromRequestParts<S>" bound " {\n"          \
    "    fn from_request(state: &S) { T::from_request_parts(state) }\n}\n"                          \
    "pub struct WithRejection<E, R>(E, R);\n"                                                      \
    "impl<E, R, S> FromRequest<S> for WithRejection<E, R> where E: FromRequest<S>" bound " {\n"    \
    "    fn from_request(state: &S) { E::from_request(state) }\n}\n"                               \
    "impl<E, R, S> FromRequestParts<S> for WithRejection<E, R> where E: FromRequestParts<S>"       \
    bound " {\n    fn from_request_parts(state: &S) { E::from_request_parts(state) }\n}\n"         \
    "pub struct TestExtractor;\npub struct TestRejection;\n"                                       \
    "impl<S> FromRequestParts<S> for TestExtractor" bound " { fn from_request_parts(_: &S) {} }\n" \
    "pub fn whole() { WithRejection::<TestExtractor, TestRejection>::from_request(&()); }\n"       \
    "pub fn parts() { WithRejection::<TestExtractor, TestRejection>::from_request_parts(&()); }\n"

static void check_issue(void) {
    const char *source = ISSUE_SHAPE(", S: Send + Sync");
    extraction_t x = extract("src/with_rejection.rs", source);
    CHECK(answers(x.e, "whole", ".WithRejection.from_request", NULL) == 0,
          "the issue: `from_request` is answered with WithRejection's own impl");
    CHECK(answers(x.e, "parts", ".WithRejection.from_request_parts", NULL) == 0,
          "the issue: `from_request_parts` is answered while `S: Send + Sync` is unknown");
    pdxe_result_free(ctx, x.r);

    /* Without the bound on `S`, `from_request_parts`'s impl is proven through
     * `TestExtractor`'s impl, whose `S` is free; `from_request`'s is not. */
    x = extract("src/with_rejection.rs", ISSUE_SHAPE(""));
    CHECK(answers(x.e, "whole", ".WithRejection.from_request", NULL) == 0,
          "the issue's shape: `from_request` is answered with WithRejection's own impl");
    CHECK(answers(x.e, "parts", ".WithRejection.from_request_parts", NULL) == 1,
          "the issue's shape: `from_request_parts` is not answered with its proven impl");
    pdxe_result_free(ctx, x.r);
}

/* The engine differential's site (axum-core/src/extract/mod.rs:129): in an impl whose
 * `T: FromRequest<S>`, `T::from_request` was answered with the blanket impl's
 * `from_request`, registered under that impl's own parameter `T`, by the name alone.
 * The blanket impl needs `T: FromRequestParts<S>` (and `S: Send + Sync`), which nothing
 * proves for the caller's `T`: no answer at all. Ratified against the reference engine
 * (decision 66, issue-109-axum-core-generic-from-request-bound). */
static void check_type_parameter_head(void) {
    extraction_t x = extract(
        "src/extract/mod.rs",
        "pub trait FromRequestParts<S> { fn from_request_parts(state: &S) -> Self; }\n"
        "pub trait FromRequest<S, M = private::ViaRequest> { fn from_request(state: &S) -> Self; }\n"
        "mod private {\n    pub enum ViaParts {}\n    pub enum ViaRequest {}\n}\n"
        "impl<S, T> FromRequest<S, private::ViaParts> for T\nwhere\n    S: Send + Sync,\n"
        "    T: FromRequestParts<S>,\n{\n    fn from_request(state: &S) -> Self {\n"
        "        Self::from_request_parts(state)\n    }\n}\n"
        "impl<S, T> FromRequest<S> for Result<T, ()>\nwhere\n    T: FromRequest<S>,\n"
        "    S: Send + Sync,\n{\n    fn from_request(state: &S) -> Self {\n"
        "        Ok(T::from_request(state))\n    }\n}\n");
    int answered = 0;
    for (int i = 0; i < x.e->resolved_calls.count; i++) {
        const PDXEResolvedCall *c = &x.e->resolved_calls.items[i];
        answered += !c->reason && ends_with(c->callee_qn, ".from_request");
    }
    CHECK(answered == 0, "the differential's site: `T::from_request` is answered (%d)", answered);
    pdxe_result_free(ctx, x.r);
}

/* Issue 117: the header crosses the interface, on a trait impl's methods with their
 * trait and on its relation, and a result rebuilt from those parts keeps them. An
 * inherent method has neither. */
static bool same_entries(const char *const *a, uint32_t n, const char *const *engine) {
    uint32_t count = 0;
    while (engine && engine[count]) {
        count++;
    }
    if (n != count || (n && !a)) {
        return false;
    }
    for (uint32_t i = 0; i < n; i++) {
        if (strcmp(a[i], engine[i]) != 0) {
            return false;
        }
    }
    return true;
}

static void check_interface(void) {
    const char *source = "trait Tr { fn f(); }\nstruct W<T>(T);\n"
                         "impl<T: Copy> Tr for W<T> { fn f() {} }\n"
                         "impl<T> W<T> { fn g() {} }\n";
    extraction_t x = extract("src/lib.rs", source);
    const PDXEImplTrait *relation = &x.e->impl_traits.items[0];
    CHECK(x.r->n_impl_traits == 1 &&
              same_entries((const char *const *)x.r->impl_traits[0].impl_bounds,
                           x.r->impl_traits[0].n_impl_bounds, relation->impl_bounds),
          "the interface: the relation's header is not the engine's");
    int methods = 0;
    for (uint32_t i = 0; i < x.r->n_defs; i++) {
        const pdxe_definition *d = &x.r->defs[i];
        if (strcmp(d->name, "f") == 0 && d->parent_index != PDXE_NO_PARENT &&
            strcmp(x.r->defs[d->parent_index].name, "W") == 0) {
            methods++;
            CHECK(d->impl_trait && strcmp(d->impl_trait, "Tr") == 0,
                  "the interface: the trait impl's method has not its trait");
            CHECK(same_entries((const char *const *)d->impl_bounds, d->n_impl_bounds,
                               relation->impl_bounds),
                  "the interface: the trait impl's method has not its impl's header");
        } else if (strcmp(d->name, "g") == 0) {
            methods++;
            CHECK(!d->impl_trait && !d->impl_bounds && d->n_impl_bounds == 0,
                  "the interface: an inherent method has a trait or a header");
        } else {
            CHECK(!d->impl_trait && d->n_impl_bounds == 0, "the interface: %s has a header",
                  d->name);
        }
    }
    CHECK(methods == 2, "the interface: the methods are missing (%d)", methods);

    /* Rebuilt from the parts a cache keeps, every one of them is kept. */
    pdxe_file_result *rebuilt = NULL;
    CHECK(pdxe_result_build(ctx, strlen(source), x.r->defs, x.r->n_defs, NULL, 0, NULL, 0, NULL, 0,
                            NULL, 0, NULL, 0, x.r->impl_traits, x.r->n_impl_traits,
                            &rebuilt) == PDXE_OK &&
              rebuilt,
          "the interface: the parts are not rebuilt");
    if (rebuilt) {
        for (uint32_t i = 0; i < rebuilt->n_defs; i++) {
            const pdxe_definition *a = &rebuilt->defs[i];
            const pdxe_definition *b = &x.r->defs[i];
            CHECK((a->impl_trait == NULL) == (b->impl_trait == NULL) &&
                      (!a->impl_trait || strcmp(a->impl_trait, b->impl_trait) == 0) &&
                      a->n_impl_bounds == b->n_impl_bounds,
                  "the interface: a rebuilt definition lost its trait or header");
            for (uint32_t k = 0; k < a->n_impl_bounds; k++) {
                CHECK(strcmp(a->impl_bounds[k], b->impl_bounds[k]) == 0,
                      "the interface: a rebuilt header entry differs");
            }
        }
        CHECK(rebuilt->n_impl_traits == 1 &&
                  rebuilt->impl_traits[0].n_impl_bounds == x.r->impl_traits[0].n_impl_bounds,
              "the interface: the rebuilt relation lost its header");
        pdxe_result_free(ctx, rebuilt);
    }
    pdxe_result_free(ctx, x.r);
}

int main(void) {
    if (pdxe_init(&ctx) != PDXE_OK) {
        printf("FAIL the engine did not start\n");
        return 1;
    }
    check_headers();
    check_answers();
    check_issue();
    check_type_parameter_head();
    check_interface();
    pdxe_shutdown(ctx);
    if (failures) {
        printf("%d failure(s)\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
