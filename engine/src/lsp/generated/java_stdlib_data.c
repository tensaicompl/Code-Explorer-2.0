/*
 * java_stdlib_data.c — Curated Java standard-library type/method registry.
 *
 * Strategy:
 *   - java.lang.* — fully covered (the implicit-import package).
 *     Object, String, StringBuilder, StringBuffer, CharSequence, Class,
 *     Throwable + the common subclass tree, Number + boxed primitives,
 *     Math, System, Thread, Iterable, Comparable, Cloneable, Enum, Record,
 *     AutoCloseable, the common Exception types.
 *   - java.util.* — collections + iterators + Optional + Date/Calendar +
 *     Arrays/Collections + Scanner/Random/UUID + Map.Entry.
 *   - java.io.* — streams, readers, writers, File, IOException family.
 *   - java.nio.file.* — Path, Paths, Files (often-used helpers).
 *   - java.util.function — the 21 functional interfaces.
 *   - java.util.stream  — Stream + Collectors entry points.
 *   - java.util.concurrent — ExecutorService, Future, CompletableFuture,
 *     ConcurrentHashMap, the concurrent collection set.
 *   - java.time — LocalDate/LocalTime/LocalDateTime/Duration/Instant.
 *
 * Method signatures use registry-level fidelity: receiver, short name,
 * return type. Param types are intentionally unmodeled (the resolver
 * chooses overloads by arity, with type compatibility scoring breaking
 * ties — see pdxe_registry_lookup_method_by_args).
 *
 * This is the JLS-spec-aligned slice of the stdlib that 90%+ of real-world
 * Java code touches.
 */

#include "../type_rep.h"
#include "../type_registry.h"
#include "../../arena.h"
#include "../java_lsp.h"
#include <string.h>

#define REG_TYPE(qn_, short_, is_iface_, parents_)            \
    do {                                                      \
        memset(&rt, 0, sizeof(rt));                           \
        rt.qualified_name = (qn_);                            \
        rt.short_name = (short_);                             \
        rt.is_interface = (is_iface_);                        \
        rt.embedded_types = (parents_);                       \
        pdxe_registry_add_type(reg, rt);                       \
    } while (0)

#define REG_METHOD(class_qn_, method_name_, ret_type_)                                          \
    do {                                                                                        \
        memset(&rf, 0, sizeof(rf));                                                             \
        rf.min_params = -1;                                                                     \
        rf.qualified_name =                                                                     \
            pdxe_arena_sprintf(arena, "%s.%s", (class_qn_), (method_name_));                     \
        rf.short_name = (method_name_);                                                         \
        rf.receiver_type = (class_qn_);                                                         \
        {                                                                                       \
            const PDXEType **rets =                                                              \
                (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(*rets));                    \
            rets[0] = (ret_type_);                                                              \
            rets[1] = NULL;                                                                     \
            rf.signature = pdxe_type_func(arena, NULL, NULL, rets);                              \
        }                                                                                       \
        pdxe_registry_add_func(reg, rf);                                                         \
    } while (0)

#define REG_CTOR(class_qn_, short_name_)                                              \
    do {                                                                              \
        memset(&rf, 0, sizeof(rf));                                                   \
        rf.min_params = -1;                                                           \
        rf.qualified_name =                                                           \
            pdxe_arena_sprintf(arena, "%s.%s", (class_qn_), (short_name_));            \
        rf.short_name = (short_name_);                                                \
        rf.receiver_type = (class_qn_);                                               \
        {                                                                             \
            const PDXEType **rets =                                                    \
                (const PDXEType **)pdxe_arena_alloc(arena, 2 * sizeof(*rets));          \
            rets[0] = pdxe_type_named(arena, (class_qn_));                             \
            rets[1] = NULL;                                                           \
            rf.signature = pdxe_type_func(arena, NULL, NULL, rets);                    \
        }                                                                             \
        pdxe_registry_add_func(reg, rf);                                               \
    } while (0)

#define REG_FIELD(class_qn_, name_, type_)                                            \
    do {                                                                              \
        const PDXERegisteredType *_existing =                                          \
            pdxe_registry_lookup_type(reg, (class_qn_));                               \
        (void)_existing;                                                              \
        /* Field append handled by REG_TYPE_FIELDS below. */                          \
        /* Placeholder for future per-field appends. */                               \
    } while (0)

void pdxe_java_stdlib_register(PDXETypeRegistry *reg, PDXEArena *arena) {
    PDXERegisteredType rt;
    PDXERegisteredFunc rf;

    /* ── Type-parent lists (must be static so addresses outlive the call) ── */
    static const char *no_parents[] = {NULL};
    static const char *parents_object[] = {"java.lang.Object", NULL};
    static const char *parents_throwable[] = {"java.lang.Object", NULL};
    static const char *parents_exception[] = {"java.lang.Throwable", NULL};
    static const char *parents_error[] = {"java.lang.Throwable", NULL};
    static const char *parents_runtime_exc[] = {"java.lang.Exception", NULL};
    static const char *parents_io_exc[] = {"java.lang.Exception", NULL};
    static const char *parents_number[] = {"java.lang.Object", NULL};
    static const char *parents_integer[] = {"java.lang.Number", NULL};
    static const char *parents_long[] = {"java.lang.Number", NULL};
    static const char *parents_double[] = {"java.lang.Number", NULL};
    static const char *parents_float[] = {"java.lang.Number", NULL};
    static const char *parents_short[] = {"java.lang.Number", NULL};
    static const char *parents_byte[] = {"java.lang.Number", NULL};
    static const char *parents_string[] = {"java.lang.Object", NULL};
    static const char *parents_charseq[] = {NULL};
    static const char *parents_iterable[] = {NULL};
    static const char *parents_collection[] = {"java.lang.Iterable", NULL};
    static const char *parents_list[] = {"java.util.Collection", NULL};
    static const char *parents_set[] = {"java.util.Collection", NULL};
    static const char *parents_queue[] = {"java.util.Collection", NULL};
    static const char *parents_deque[] = {"java.util.Queue", NULL};
    static const char *parents_map[] = {NULL};
    static const char *parents_map_entry[] = {NULL};
    static const char *parents_iterator[] = {NULL};
    static const char *parents_arraylist[] = {"java.util.List", NULL};
    static const char *parents_linkedlist[] = {"java.util.List", NULL};
    static const char *parents_hashset[] = {"java.util.Set", NULL};
    static const char *parents_treeset[] = {"java.util.Set", NULL};
    static const char *parents_linkedhashset[] = {"java.util.Set", NULL};
    static const char *parents_hashmap[] = {"java.util.Map", NULL};
    static const char *parents_treemap[] = {"java.util.Map", NULL};
    static const char *parents_linkedhashmap[] = {"java.util.Map", NULL};
    static const char *parents_concurrent_hashmap[] = {"java.util.Map", NULL};

    static const char *parents_inputstream[] = {"java.lang.AutoCloseable", NULL};
    static const char *parents_outputstream[] = {"java.lang.AutoCloseable", NULL};
    static const char *parents_reader[] = {"java.lang.AutoCloseable", NULL};
    static const char *parents_writer[] = {"java.lang.AutoCloseable", NULL};
    static const char *parents_buffered_reader[] = {"java.io.Reader", NULL};
    static const char *parents_buffered_writer[] = {"java.io.Writer", NULL};
    static const char *parents_print_stream[] = {"java.io.OutputStream", NULL};
    static const char *parents_print_writer[] = {"java.io.Writer", NULL};
    static const char *parents_file_input_stream[] = {"java.io.InputStream", NULL};
    static const char *parents_file_output_stream[] = {"java.io.OutputStream", NULL};
    static const char *parents_file_reader[] = {"java.io.Reader", NULL};
    static const char *parents_file_writer[] = {"java.io.Writer", NULL};
    static const char *parents_io_exception[] = {"java.lang.Exception", NULL};
    static const char *parents_runtime_exc_chain[] = {"java.lang.RuntimeException", NULL};
    /* Parent lists for types previously registered with inline compound
     * literals. A compound literal has automatic (block) storage duration,
     * so storing its address into the registry left a dangling stack pointer
     * once the REG_TYPE statement's block ended — an AddressSanitizer
     * stack-use-after-scope when the inheritance walk later read
     * rt->embedded_types[0]. These must be static so their addresses outlive
     * the call, exactly like the parent lists above. */
    static const char *parents_gregorian_calendar[] = {"java.util.Calendar", NULL};
    static const char *parents_file_not_found_exc[] = {"java.io.IOException", NULL};
    static const char *parents_closeable[] = {"java.lang.AutoCloseable", NULL};
    static const char *parents_unary_operator[] = {"java.util.function.Function", NULL};
    static const char *parents_binary_operator[] = {"java.util.function.BiFunction", NULL};
    static const char *parents_completable_future[] = {"java.util.concurrent.Future", NULL};
    static const char *parents_reentrant_lock[] = {"java.util.concurrent.locks.Lock", NULL};

    /* ── java.lang ─────────────────────────────────────────────── */
    REG_TYPE("java.lang.Object", "Object", false, no_parents);
    REG_TYPE("java.lang.Class", "Class", false, parents_object);
    REG_TYPE("java.lang.ClassLoader", "ClassLoader", false, parents_object);
    REG_TYPE("java.lang.CharSequence", "CharSequence", true, parents_charseq);
    REG_TYPE("java.lang.String", "String", false, parents_string);
    REG_TYPE("java.lang.StringBuilder", "StringBuilder", false, parents_object);
    REG_TYPE("java.lang.StringBuffer", "StringBuffer", false, parents_object);
    REG_TYPE("java.lang.Number", "Number", false, parents_number);
    REG_TYPE("java.lang.Integer", "Integer", false, parents_integer);
    REG_TYPE("java.lang.Long", "Long", false, parents_long);
    REG_TYPE("java.lang.Short", "Short", false, parents_short);
    REG_TYPE("java.lang.Byte", "Byte", false, parents_byte);
    REG_TYPE("java.lang.Float", "Float", false, parents_float);
    REG_TYPE("java.lang.Double", "Double", false, parents_double);
    REG_TYPE("java.lang.Boolean", "Boolean", false, parents_object);
    REG_TYPE("java.lang.Character", "Character", false, parents_object);
    REG_TYPE("java.lang.Void", "Void", false, parents_object);
    REG_TYPE("java.lang.Iterable", "Iterable", true, parents_iterable);
    REG_TYPE("java.lang.Comparable", "Comparable", true, no_parents);
    REG_TYPE("java.lang.Cloneable", "Cloneable", true, no_parents);
    REG_TYPE("java.lang.Runnable", "Runnable", true, no_parents);
    REG_TYPE("java.lang.AutoCloseable", "AutoCloseable", true, no_parents);
    REG_TYPE("java.lang.Math", "Math", false, parents_object);
    REG_TYPE("java.lang.System", "System", false, parents_object);
    REG_TYPE("java.lang.Thread", "Thread", false, parents_object);
    REG_TYPE("java.lang.Process", "Process", false, parents_object);
    REG_TYPE("java.lang.ProcessBuilder", "ProcessBuilder", false, parents_object);
    REG_TYPE("java.lang.StackTraceElement", "StackTraceElement", false, parents_object);
    REG_TYPE("java.lang.Enum", "Enum", false, parents_object);
    REG_TYPE("java.lang.Record", "Record", false, parents_object);
    REG_TYPE("java.lang.Throwable", "Throwable", false, parents_throwable);
    REG_TYPE("java.lang.Exception", "Exception", false, parents_exception);
    REG_TYPE("java.lang.Error", "Error", false, parents_error);
    REG_TYPE("java.lang.RuntimeException", "RuntimeException", false, parents_runtime_exc);
    REG_TYPE("java.lang.NullPointerException", "NullPointerException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.IllegalArgumentException", "IllegalArgumentException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.IllegalStateException", "IllegalStateException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.IndexOutOfBoundsException", "IndexOutOfBoundsException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.ArrayIndexOutOfBoundsException", "ArrayIndexOutOfBoundsException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.ArithmeticException", "ArithmeticException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.ClassCastException", "ClassCastException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.ClassNotFoundException", "ClassNotFoundException", false,
             parents_exception);
    REG_TYPE("java.lang.NumberFormatException", "NumberFormatException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.UnsupportedOperationException", "UnsupportedOperationException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.InterruptedException", "InterruptedException", false, parents_exception);
    REG_TYPE("java.lang.SecurityException", "SecurityException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.lang.NoSuchMethodException", "NoSuchMethodException", false, parents_exception);
    REG_TYPE("java.lang.NoSuchFieldException", "NoSuchFieldException", false, parents_exception);

    /* Annotation-marker types. */
    REG_TYPE("java.lang.Override", "Override", true, no_parents);
    REG_TYPE("java.lang.Deprecated", "Deprecated", true, no_parents);
    REG_TYPE("java.lang.SuppressWarnings", "SuppressWarnings", true, no_parents);
    REG_TYPE("java.lang.FunctionalInterface", "FunctionalInterface", true, no_parents);

    /* ── Object methods ───────────────────────────────────────── */
    REG_METHOD("java.lang.Object", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Object", "hashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Object", "equals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Object", "getClass", pdxe_type_named(arena, "java.lang.Class"));
    REG_METHOD("java.lang.Object", "wait", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Object", "notify", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Object", "notifyAll", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Object", "clone", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.lang.Object", "finalize", pdxe_type_builtin(arena, "void"));

    /* ── String methods ───────────────────────────────────────── */
    REG_METHOD("java.lang.String", "length", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "isBlank", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "charAt", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.String", "codePointAt", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "equals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "equalsIgnoreCase", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "compareTo", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "compareToIgnoreCase", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "indexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "lastIndexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "contains", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "startsWith", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "endsWith", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "matches", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.String", "concat", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "substring", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "trim", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "strip", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "stripLeading", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "stripTrailing", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "toLowerCase", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "toUpperCase", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "replace", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "replaceAll", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "replaceFirst", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "split",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.String")));
    REG_METHOD("java.lang.String", "toCharArray", pdxe_type_slice(arena, pdxe_type_builtin(arena, "char")));
    REG_METHOD("java.lang.String", "getBytes", pdxe_type_slice(arena, pdxe_type_builtin(arena, "byte")));
    REG_METHOD("java.lang.String", "intern", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "format", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "valueOf", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "join", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "repeat", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "lines", pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.lang.String", "chars", pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.lang.String", "codePoints",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.lang.String", "hashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.String", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.String", "toCharArray", pdxe_type_slice(arena, pdxe_type_builtin(arena, "char")));
    REG_CTOR("java.lang.String", "String");

    /* ── StringBuilder / StringBuffer ─────────────────────────── */
    REG_METHOD("java.lang.StringBuilder", "append",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "insert",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "delete",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "deleteCharAt",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "replace",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "reverse",
               pdxe_type_named(arena, "java.lang.StringBuilder"));
    REG_METHOD("java.lang.StringBuilder", "toString",
               pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.StringBuilder", "length", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.StringBuilder", "charAt", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.StringBuilder", "setLength", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.StringBuilder", "indexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.StringBuilder", "substring",
               pdxe_type_named(arena, "java.lang.String"));
    REG_CTOR("java.lang.StringBuilder", "StringBuilder");

    REG_METHOD("java.lang.StringBuffer", "append",
               pdxe_type_named(arena, "java.lang.StringBuffer"));
    REG_METHOD("java.lang.StringBuffer", "toString",
               pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.StringBuffer", "length", pdxe_type_builtin(arena, "int"));
    REG_CTOR("java.lang.StringBuffer", "StringBuffer");

    /* ── CharSequence ─────────────────────────────────────────── */
    REG_METHOD("java.lang.CharSequence", "length", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.CharSequence", "charAt", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.CharSequence", "subSequence",
               pdxe_type_named(arena, "java.lang.CharSequence"));
    REG_METHOD("java.lang.CharSequence", "toString",
               pdxe_type_named(arena, "java.lang.String"));

    /* ── Number + boxed types ─────────────────────────────────── */
    REG_METHOD("java.lang.Number", "intValue", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Number", "longValue", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Number", "doubleValue", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Number", "floatValue", pdxe_type_builtin(arena, "float"));
    REG_METHOD("java.lang.Number", "shortValue", pdxe_type_builtin(arena, "short"));
    REG_METHOD("java.lang.Number", "byteValue", pdxe_type_builtin(arena, "byte"));

    REG_METHOD("java.lang.Integer", "intValue", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "parseInt", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "valueOf", pdxe_type_named(arena, "java.lang.Integer"));
    REG_METHOD("java.lang.Integer", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Integer", "compare", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "compareTo", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "equals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Integer", "hashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "max", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "min", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "sum", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "bitCount", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Integer", "toBinaryString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Integer", "toHexString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Integer", "toOctalString", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.lang.Long", "longValue", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Long", "parseLong", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Long", "valueOf", pdxe_type_named(arena, "java.lang.Long"));
    REG_METHOD("java.lang.Long", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Long", "compareTo", pdxe_type_builtin(arena, "int"));

    REG_METHOD("java.lang.Double", "doubleValue", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Double", "parseDouble", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Double", "valueOf", pdxe_type_named(arena, "java.lang.Double"));
    REG_METHOD("java.lang.Double", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Double", "isNaN", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Double", "isInfinite", pdxe_type_builtin(arena, "boolean"));

    REG_METHOD("java.lang.Float", "floatValue", pdxe_type_builtin(arena, "float"));
    REG_METHOD("java.lang.Float", "parseFloat", pdxe_type_builtin(arena, "float"));
    REG_METHOD("java.lang.Float", "valueOf", pdxe_type_named(arena, "java.lang.Float"));

    REG_METHOD("java.lang.Boolean", "booleanValue", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Boolean", "parseBoolean", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Boolean", "valueOf", pdxe_type_named(arena, "java.lang.Boolean"));
    REG_METHOD("java.lang.Boolean", "toString", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.lang.Character", "charValue", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.Character", "isDigit", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "isLetter", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "isLetterOrDigit", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "isWhitespace", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "isUpperCase", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "isLowerCase", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Character", "toUpperCase", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.Character", "toLowerCase", pdxe_type_builtin(arena, "char"));
    REG_METHOD("java.lang.Character", "getNumericValue", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.Character", "valueOf", pdxe_type_named(arena, "java.lang.Character"));

    REG_METHOD("java.lang.Byte", "byteValue", pdxe_type_builtin(arena, "byte"));
    REG_METHOD("java.lang.Byte", "parseByte", pdxe_type_builtin(arena, "byte"));
    REG_METHOD("java.lang.Byte", "valueOf", pdxe_type_named(arena, "java.lang.Byte"));

    REG_METHOD("java.lang.Short", "shortValue", pdxe_type_builtin(arena, "short"));
    REG_METHOD("java.lang.Short", "parseShort", pdxe_type_builtin(arena, "short"));
    REG_METHOD("java.lang.Short", "valueOf", pdxe_type_named(arena, "java.lang.Short"));

    /* ── Math ─────────────────────────────────────────────────── */
    REG_METHOD("java.lang.Math", "abs", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "min", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "max", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "sqrt", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "cbrt", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "pow", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "exp", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "log", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "log10", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "sin", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "cos", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "tan", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "asin", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "acos", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "atan", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "atan2", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "floor", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "ceil", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "round", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "random", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "signum", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "hypot", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "floorDiv", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "floorMod", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "addExact", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "subtractExact", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "multiplyExact", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Math", "toRadians", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.lang.Math", "toDegrees", pdxe_type_builtin(arena, "double"));

    /* ── System ───────────────────────────────────────────────── */
    REG_METHOD("java.lang.System", "currentTimeMillis", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.System", "nanoTime", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.System", "exit", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.System", "getenv", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.System", "getProperty", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.System", "setProperty", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.System", "lineSeparator", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.System", "arraycopy", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.System", "identityHashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.lang.System", "gc", pdxe_type_builtin(arena, "void"));

    /* ── Thread ───────────────────────────────────────────────── */
    REG_METHOD("java.lang.Thread", "start", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "run", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "join", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "interrupt", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "isAlive", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Thread", "sleep", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "currentThread", pdxe_type_named(arena, "java.lang.Thread"));
    REG_METHOD("java.lang.Thread", "yield", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "getName", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Thread", "setName", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Thread", "getId", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.lang.Thread", "isInterrupted", pdxe_type_builtin(arena, "boolean"));

    /* ── Class ────────────────────────────────────────────────── */
    REG_METHOD("java.lang.Class", "getName", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Class", "getSimpleName", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Class", "getCanonicalName", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Class", "isInterface", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Class", "isArray", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Class", "isAssignableFrom", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Class", "isInstance", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.lang.Class", "newInstance", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.lang.Class", "forName", pdxe_type_named(arena, "java.lang.Class"));
    REG_METHOD("java.lang.Class", "getSuperclass", pdxe_type_named(arena, "java.lang.Class"));
    REG_METHOD("java.lang.Class", "getInterfaces",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Class")));

    /* ── Iterable / Iterator ──────────────────────────────────── */
    REG_METHOD("java.lang.Iterable", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.lang.Iterable", "forEach", pdxe_type_builtin(arena, "void"));

    /* ── Throwable methods ────────────────────────────────────── */
    REG_METHOD("java.lang.Throwable", "getMessage", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Throwable", "getLocalizedMessage",
               pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Throwable", "getCause", pdxe_type_named(arena, "java.lang.Throwable"));
    REG_METHOD("java.lang.Throwable", "initCause",
               pdxe_type_named(arena, "java.lang.Throwable"));
    REG_METHOD("java.lang.Throwable", "printStackTrace", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.lang.Throwable", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.lang.Throwable", "getStackTrace",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.StackTraceElement")));

    /* ── AutoCloseable ────────────────────────────────────────── */
    REG_METHOD("java.lang.AutoCloseable", "close", pdxe_type_builtin(arena, "void"));

    /* ── Comparable ───────────────────────────────────────────── */
    REG_METHOD("java.lang.Comparable", "compareTo", pdxe_type_builtin(arena, "int"));

    /* ── Runnable ─────────────────────────────────────────────── */
    REG_METHOD("java.lang.Runnable", "run", pdxe_type_builtin(arena, "void"));

    /* ── java.util ────────────────────────────────────────────── */
    REG_TYPE("java.util.Collection", "Collection", true, parents_collection);
    REG_TYPE("java.util.List", "List", true, parents_list);
    REG_TYPE("java.util.Set", "Set", true, parents_set);
    REG_TYPE("java.util.Queue", "Queue", true, parents_queue);
    REG_TYPE("java.util.Deque", "Deque", true, parents_deque);
    REG_TYPE("java.util.Map", "Map", true, parents_map);
    REG_TYPE("java.util.Map.Entry", "Entry", true, parents_map_entry);
    REG_TYPE("java.util.Iterator", "Iterator", true, parents_iterator);
    REG_TYPE("java.util.ListIterator", "ListIterator", true, parents_iterator);
    REG_TYPE("java.util.Spliterator", "Spliterator", true, no_parents);
    REG_TYPE("java.util.Comparator", "Comparator", true, no_parents);

    REG_TYPE("java.util.ArrayList", "ArrayList", false, parents_arraylist);
    REG_TYPE("java.util.LinkedList", "LinkedList", false, parents_linkedlist);
    REG_TYPE("java.util.Vector", "Vector", false, parents_arraylist);
    REG_TYPE("java.util.Stack", "Stack", false, parents_arraylist);
    REG_TYPE("java.util.HashSet", "HashSet", false, parents_hashset);
    REG_TYPE("java.util.TreeSet", "TreeSet", false, parents_treeset);
    REG_TYPE("java.util.LinkedHashSet", "LinkedHashSet", false, parents_linkedhashset);
    REG_TYPE("java.util.HashMap", "HashMap", false, parents_hashmap);
    REG_TYPE("java.util.TreeMap", "TreeMap", false, parents_treemap);
    REG_TYPE("java.util.LinkedHashMap", "LinkedHashMap", false, parents_linkedhashmap);
    REG_TYPE("java.util.ArrayDeque", "ArrayDeque", false, parents_deque);
    REG_TYPE("java.util.PriorityQueue", "PriorityQueue", false, parents_queue);

    REG_TYPE("java.util.Optional", "Optional", false, parents_object);
    REG_TYPE("java.util.OptionalInt", "OptionalInt", false, parents_object);
    REG_TYPE("java.util.OptionalLong", "OptionalLong", false, parents_object);
    REG_TYPE("java.util.OptionalDouble", "OptionalDouble", false, parents_object);
    REG_TYPE("java.util.Date", "Date", false, parents_object);
    REG_TYPE("java.util.Calendar", "Calendar", false, parents_object);
    REG_TYPE("java.util.GregorianCalendar", "GregorianCalendar", false,
             parents_gregorian_calendar);
    REG_TYPE("java.util.TimeZone", "TimeZone", false, parents_object);
    REG_TYPE("java.util.Locale", "Locale", false, parents_object);
    REG_TYPE("java.util.UUID", "UUID", false, parents_object);
    REG_TYPE("java.util.Random", "Random", false, parents_object);
    REG_TYPE("java.util.Scanner", "Scanner", false, parents_object);
    REG_TYPE("java.util.Arrays", "Arrays", false, parents_object);
    REG_TYPE("java.util.Collections", "Collections", false, parents_object);
    REG_TYPE("java.util.Objects", "Objects", false, parents_object);
    REG_TYPE("java.util.Properties", "Properties", false, parents_hashmap);
    REG_TYPE("java.util.regex.Pattern", "Pattern", false, parents_object);
    REG_TYPE("java.util.regex.Matcher", "Matcher", false, parents_object);

    /* ── Collection methods ───────────────────────────────────── */
    REG_METHOD("java.util.Collection", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Collection", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "contains", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "containsAll", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.util.Collection", "toArray",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Object")));
    REG_METHOD("java.util.Collection", "add", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "addAll", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "remove", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "removeAll", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "retainAll", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Collection", "clear", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Collection", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.Collection", "parallelStream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.Collection", "forEach", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Collection", "removeIf", pdxe_type_builtin(arena, "boolean"));

    /* ── List methods ─────────────────────────────────────────── */
    REG_METHOD("java.util.List", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.List", "set", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.List", "add", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.List", "remove", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.List", "indexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.List", "lastIndexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.List", "subList", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.List", "of", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.List", "copyOf", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.List", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.List", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.List", "contains", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.List", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.util.List", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.List", "forEach", pdxe_type_builtin(arena, "void"));

    /* ── ArrayList ────────────────────────────────────────────── */
    REG_METHOD("java.util.ArrayList", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.ArrayList", "set", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.ArrayList", "add", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.ArrayList", "remove", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.ArrayList", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.ArrayList", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.ArrayList", "indexOf", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.ArrayList", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.util.ArrayList", "clear", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.ArrayList", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.ArrayList", "toArray",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Object")));
    REG_METHOD("java.util.ArrayList", "subList", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.ArrayList", "trimToSize", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.ArrayList", "ensureCapacity", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.ArrayList", "forEach", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.ArrayList", "removeIf", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.List", "removeIf", pdxe_type_builtin(arena, "boolean"));
    REG_CTOR("java.util.ArrayList", "ArrayList");

    REG_METHOD("java.util.LinkedList", "addFirst", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.LinkedList", "addLast", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.LinkedList", "removeFirst", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "removeLast", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "getFirst", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "getLast", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "peek", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "poll", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.LinkedList", "offer", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.LinkedList", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.LinkedList", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_CTOR("java.util.LinkedList", "LinkedList");

    /* ── Set methods ──────────────────────────────────────────── */
    REG_METHOD("java.util.Set", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Set", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Set", "contains", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Set", "add", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Set", "remove", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Set", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.util.Set", "of", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Set", "copyOf", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Set", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.Set", "forEach", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.util.HashSet", "add", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashSet", "remove", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashSet", "contains", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashSet", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.HashSet", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashSet", "iterator", pdxe_type_named(arena, "java.util.Iterator"));
    REG_METHOD("java.util.HashSet", "clear", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.HashSet", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_CTOR("java.util.HashSet", "HashSet");

    REG_METHOD("java.util.TreeSet", "first", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.TreeSet", "last", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.TreeSet", "headSet", pdxe_type_named(arena, "java.util.SortedSet"));
    REG_METHOD("java.util.TreeSet", "tailSet", pdxe_type_named(arena, "java.util.SortedSet"));
    REG_CTOR("java.util.TreeSet", "TreeSet");

    REG_CTOR("java.util.LinkedHashSet", "LinkedHashSet");

    /* ── Map methods ──────────────────────────────────────────── */
    REG_METHOD("java.util.Map", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "put", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "remove", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "containsKey", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Map", "containsValue", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Map", "keySet", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Map", "values", pdxe_type_named(arena, "java.util.Collection"));
    REG_METHOD("java.util.Map", "entrySet", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Map", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Map", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Map", "putAll", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Map", "clear", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Map", "getOrDefault", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "putIfAbsent", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "computeIfAbsent", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "compute", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "merge", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map", "of", pdxe_type_named(arena, "java.util.Map"));
    REG_METHOD("java.util.Map", "copyOf", pdxe_type_named(arena, "java.util.Map"));
    REG_METHOD("java.util.Map", "ofEntries", pdxe_type_named(arena, "java.util.Map"));
    REG_METHOD("java.util.Map", "entry", pdxe_type_named(arena, "java.util.Map.Entry"));
    REG_METHOD("java.util.Map", "forEach", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.util.Map.Entry", "getKey", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map.Entry", "getValue", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Map.Entry", "setValue", pdxe_type_named(arena, "java.lang.Object"));

    REG_METHOD("java.util.HashMap", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.HashMap", "put", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.HashMap", "remove", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.HashMap", "containsKey", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashMap", "containsValue", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashMap", "size", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.HashMap", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.HashMap", "keySet", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.HashMap", "values", pdxe_type_named(arena, "java.util.Collection"));
    REG_METHOD("java.util.HashMap", "entrySet", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.HashMap", "clear", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.HashMap", "getOrDefault", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.HashMap", "putIfAbsent", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.HashMap", "forEach", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.util.HashMap", "HashMap");

    REG_METHOD("java.util.TreeMap", "firstKey", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.TreeMap", "lastKey", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.TreeMap", "headMap", pdxe_type_named(arena, "java.util.SortedMap"));
    REG_METHOD("java.util.TreeMap", "tailMap", pdxe_type_named(arena, "java.util.SortedMap"));
    REG_CTOR("java.util.TreeMap", "TreeMap");

    REG_CTOR("java.util.LinkedHashMap", "LinkedHashMap");

    /* ── Iterator methods ─────────────────────────────────────── */
    REG_METHOD("java.util.Iterator", "hasNext", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Iterator", "next", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Iterator", "remove", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Iterator", "forEachRemaining", pdxe_type_builtin(arena, "void"));

    /* ── Optional ─────────────────────────────────────────────── */
    REG_METHOD("java.util.Optional", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Optional", "isPresent", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Optional", "isEmpty", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Optional", "orElse", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Optional", "orElseGet", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Optional", "orElseThrow", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Optional", "ifPresent", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Optional", "ifPresentOrElse", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Optional", "map", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "flatMap", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "filter", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "of", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "ofNullable", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "empty", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.Optional", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));

    /* ── Arrays / Collections / Objects helpers ───────────────── */
    REG_METHOD("java.util.Arrays", "asList", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.Arrays", "stream",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.Arrays", "sort", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Arrays", "binarySearch", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Arrays", "fill", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Arrays", "copyOf",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Object")));
    REG_METHOD("java.util.Arrays", "copyOfRange",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Object")));
    REG_METHOD("java.util.Arrays", "equals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Arrays", "hashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Arrays", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Arrays", "deepEquals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Arrays", "deepToString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Arrays", "deepHashCode", pdxe_type_builtin(arena, "int"));

    REG_METHOD("java.util.Collections", "sort", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Collections", "reverse", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Collections", "shuffle", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Collections", "min", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Collections", "max", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Collections", "emptyList", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.Collections", "emptySet", pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Collections", "emptyMap", pdxe_type_named(arena, "java.util.Map"));
    REG_METHOD("java.util.Collections", "singletonList",
               pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.Collections", "singleton",
               pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Collections", "unmodifiableList",
               pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.Collections", "unmodifiableSet",
               pdxe_type_named(arena, "java.util.Set"));
    REG_METHOD("java.util.Collections", "unmodifiableMap",
               pdxe_type_named(arena, "java.util.Map"));
    REG_METHOD("java.util.Collections", "frequency", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Collections", "binarySearch", pdxe_type_builtin(arena, "int"));

    REG_METHOD("java.util.Objects", "equals", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Objects", "hashCode", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Objects", "hash", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Objects", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Objects", "isNull", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Objects", "nonNull", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Objects", "requireNonNull", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.Objects", "requireNonNullElse",
               pdxe_type_named(arena, "java.lang.Object"));

    /* ── UUID, Random, Scanner ────────────────────────────────── */
    REG_METHOD("java.util.UUID", "randomUUID", pdxe_type_named(arena, "java.util.UUID"));
    REG_METHOD("java.util.UUID", "fromString", pdxe_type_named(arena, "java.util.UUID"));
    REG_METHOD("java.util.UUID", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.UUID", "getMostSignificantBits", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.UUID", "getLeastSignificantBits", pdxe_type_builtin(arena, "long"));
    REG_CTOR("java.util.UUID", "UUID");

    REG_METHOD("java.util.Random", "nextInt", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Random", "nextLong", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.Random", "nextDouble", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.util.Random", "nextFloat", pdxe_type_builtin(arena, "float"));
    REG_METHOD("java.util.Random", "nextBoolean", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Random", "nextGaussian", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.util.Random", "setSeed", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.util.Random", "Random");

    REG_METHOD("java.util.Scanner", "next", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Scanner", "nextLine", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Scanner", "nextInt", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Scanner", "nextLong", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.Scanner", "nextDouble", pdxe_type_builtin(arena, "double"));
    REG_METHOD("java.util.Scanner", "hasNext", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Scanner", "hasNextLine", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Scanner", "hasNextInt", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Scanner", "close", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.util.Scanner", "Scanner");

    /* ── Locale / Date / Calendar / TimeZone ──────────────────── */
    REG_METHOD("java.util.Locale", "getLanguage", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Locale", "getCountry", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Locale", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.Locale", "getDefault", pdxe_type_named(arena, "java.util.Locale"));
    REG_CTOR("java.util.Locale", "Locale");

    REG_METHOD("java.util.Date", "getTime", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.Date", "setTime", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Date", "before", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Date", "after", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.Date", "compareTo", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Date", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_CTOR("java.util.Date", "Date");

    REG_METHOD("java.util.Calendar", "getInstance", pdxe_type_named(arena, "java.util.Calendar"));
    REG_METHOD("java.util.Calendar", "getTime", pdxe_type_named(arena, "java.util.Date"));
    REG_METHOD("java.util.Calendar", "set", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.Calendar", "get", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.Calendar", "add", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.util.TimeZone", "getDefault", pdxe_type_named(arena, "java.util.TimeZone"));
    REG_METHOD("java.util.TimeZone", "getID", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.TimeZone", "getTimeZone", pdxe_type_named(arena, "java.util.TimeZone"));

    /* ── regex ────────────────────────────────────────────────── */
    REG_METHOD("java.util.regex.Pattern", "compile",
               pdxe_type_named(arena, "java.util.regex.Pattern"));
    REG_METHOD("java.util.regex.Pattern", "matcher",
               pdxe_type_named(arena, "java.util.regex.Matcher"));
    REG_METHOD("java.util.regex.Pattern", "matches", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.regex.Pattern", "split",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.String")));
    REG_METHOD("java.util.regex.Pattern", "pattern", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.util.regex.Matcher", "matches", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.regex.Matcher", "find", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.regex.Matcher", "group", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.regex.Matcher", "groupCount", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.regex.Matcher", "start", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.regex.Matcher", "end", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.regex.Matcher", "replaceAll", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.util.regex.Matcher", "replaceFirst",
               pdxe_type_named(arena, "java.lang.String"));

    /* ── java.io ──────────────────────────────────────────────── */
    REG_TYPE("java.io.InputStream", "InputStream", false, parents_inputstream);
    REG_TYPE("java.io.OutputStream", "OutputStream", false, parents_outputstream);
    REG_TYPE("java.io.Reader", "Reader", false, parents_reader);
    REG_TYPE("java.io.Writer", "Writer", false, parents_writer);
    REG_TYPE("java.io.BufferedReader", "BufferedReader", false, parents_buffered_reader);
    REG_TYPE("java.io.BufferedWriter", "BufferedWriter", false, parents_buffered_writer);
    REG_TYPE("java.io.PrintStream", "PrintStream", false, parents_print_stream);
    REG_TYPE("java.io.PrintWriter", "PrintWriter", false, parents_print_writer);
    REG_TYPE("java.io.FileInputStream", "FileInputStream", false, parents_file_input_stream);
    REG_TYPE("java.io.FileOutputStream", "FileOutputStream", false, parents_file_output_stream);
    REG_TYPE("java.io.FileReader", "FileReader", false, parents_file_reader);
    REG_TYPE("java.io.FileWriter", "FileWriter", false, parents_file_writer);
    REG_TYPE("java.io.File", "File", false, parents_object);
    REG_TYPE("java.io.IOException", "IOException", false, parents_io_exception);
    REG_TYPE("java.io.FileNotFoundException", "FileNotFoundException", false,
             parents_file_not_found_exc);
    REG_TYPE("java.io.UncheckedIOException", "UncheckedIOException", false,
             parents_runtime_exc_chain);
    REG_TYPE("java.io.Serializable", "Serializable", true, no_parents);
    REG_TYPE("java.io.Closeable", "Closeable", true,
             parents_closeable);
    REG_TYPE("java.io.Flushable", "Flushable", true, no_parents);

    REG_METHOD("java.io.PrintStream", "println", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintStream", "print", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintStream", "printf", pdxe_type_named(arena, "java.io.PrintStream"));
    REG_METHOD("java.io.PrintStream", "format", pdxe_type_named(arena, "java.io.PrintStream"));
    REG_METHOD("java.io.PrintStream", "write", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintStream", "flush", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintStream", "close", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.io.PrintWriter", "println", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintWriter", "print", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintWriter", "printf", pdxe_type_named(arena, "java.io.PrintWriter"));
    REG_METHOD("java.io.PrintWriter", "flush", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.PrintWriter", "close", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.io.PrintWriter", "PrintWriter");

    REG_METHOD("java.io.InputStream", "read", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.io.InputStream", "close", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.InputStream", "available", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.io.InputStream", "skip", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.io.InputStream", "readAllBytes",
               pdxe_type_slice(arena, pdxe_type_builtin(arena, "byte")));

    REG_METHOD("java.io.OutputStream", "write", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.OutputStream", "flush", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.OutputStream", "close", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.io.Reader", "read", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.io.Reader", "close", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.Reader", "ready", pdxe_type_builtin(arena, "boolean"));

    REG_METHOD("java.io.Writer", "write", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.Writer", "flush", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.Writer", "close", pdxe_type_builtin(arena, "void"));

    REG_METHOD("java.io.BufferedReader", "readLine", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.BufferedReader", "lines",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.io.BufferedReader", "close", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.io.BufferedReader", "BufferedReader");

    REG_METHOD("java.io.BufferedWriter", "write", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.BufferedWriter", "newLine", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.BufferedWriter", "flush", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.io.BufferedWriter", "close", pdxe_type_builtin(arena, "void"));
    REG_CTOR("java.io.BufferedWriter", "BufferedWriter");

    REG_METHOD("java.io.File", "exists", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "isFile", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "isDirectory", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "canRead", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "canWrite", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "getName", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.File", "getPath", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.File", "getAbsolutePath", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.File", "getCanonicalPath", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.File", "getParent", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.io.File", "getParentFile", pdxe_type_named(arena, "java.io.File"));
    REG_METHOD("java.io.File", "length", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.io.File", "lastModified", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.io.File", "mkdir", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "mkdirs", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "delete", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "renameTo", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.io.File", "list",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.String")));
    REG_METHOD("java.io.File", "listFiles",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.io.File")));
    REG_METHOD("java.io.File", "toPath", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.io.File", "toURI", pdxe_type_named(arena, "java.net.URI"));
    REG_CTOR("java.io.File", "File");

    /* ── java.nio.file ───────────────────────────────────────── */
    REG_TYPE("java.nio.file.Path", "Path", true, no_parents);
    REG_TYPE("java.nio.file.Paths", "Paths", false, parents_object);
    REG_TYPE("java.nio.file.Files", "Files", false, parents_object);

    REG_METHOD("java.nio.file.Path", "getFileName", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "getParent", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "getRoot", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "resolve", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "resolveSibling",
               pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "relativize", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "normalize", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "toAbsolutePath",
               pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Path", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.nio.file.Path", "toFile", pdxe_type_named(arena, "java.io.File"));
    REG_METHOD("java.nio.file.Path", "of", pdxe_type_named(arena, "java.nio.file.Path"));

    REG_METHOD("java.nio.file.Paths", "get", pdxe_type_named(arena, "java.nio.file.Path"));

    REG_METHOD("java.nio.file.Files", "exists", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.nio.file.Files", "isDirectory", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.nio.file.Files", "isRegularFile", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.nio.file.Files", "readString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.nio.file.Files", "writeString", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "readAllLines", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.nio.file.Files", "readAllBytes",
               pdxe_type_slice(arena, pdxe_type_builtin(arena, "byte")));
    REG_METHOD("java.nio.file.Files", "lines",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.nio.file.Files", "list",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.nio.file.Files", "walk",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.nio.file.Files", "createDirectory",
               pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "createDirectories",
               pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "createFile",
               pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "delete", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.nio.file.Files", "deleteIfExists", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.nio.file.Files", "copy", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "move", pdxe_type_named(arena, "java.nio.file.Path"));
    REG_METHOD("java.nio.file.Files", "size", pdxe_type_builtin(arena, "long"));

    /* ── java.util.function (the 21 functional interfaces) ──── */
    REG_TYPE("java.util.function.Function", "Function", true, no_parents);
    REG_TYPE("java.util.function.BiFunction", "BiFunction", true, no_parents);
    REG_TYPE("java.util.function.Predicate", "Predicate", true, no_parents);
    REG_TYPE("java.util.function.BiPredicate", "BiPredicate", true, no_parents);
    REG_TYPE("java.util.function.Consumer", "Consumer", true, no_parents);
    REG_TYPE("java.util.function.BiConsumer", "BiConsumer", true, no_parents);
    REG_TYPE("java.util.function.Supplier", "Supplier", true, no_parents);
    REG_TYPE("java.util.function.UnaryOperator", "UnaryOperator", true,
             parents_unary_operator);
    REG_TYPE("java.util.function.BinaryOperator", "BinaryOperator", true,
             parents_binary_operator);
    REG_TYPE("java.util.function.IntFunction", "IntFunction", true, no_parents);
    REG_TYPE("java.util.function.LongFunction", "LongFunction", true, no_parents);
    REG_TYPE("java.util.function.DoubleFunction", "DoubleFunction", true, no_parents);
    REG_TYPE("java.util.function.IntPredicate", "IntPredicate", true, no_parents);
    REG_TYPE("java.util.function.LongPredicate", "LongPredicate", true, no_parents);
    REG_TYPE("java.util.function.DoublePredicate", "DoublePredicate", true, no_parents);
    REG_TYPE("java.util.function.IntConsumer", "IntConsumer", true, no_parents);
    REG_TYPE("java.util.function.LongConsumer", "LongConsumer", true, no_parents);
    REG_TYPE("java.util.function.DoubleConsumer", "DoubleConsumer", true, no_parents);
    REG_TYPE("java.util.function.IntSupplier", "IntSupplier", true, no_parents);
    REG_TYPE("java.util.function.LongSupplier", "LongSupplier", true, no_parents);
    REG_TYPE("java.util.function.DoubleSupplier", "DoubleSupplier", true, no_parents);
    REG_TYPE("java.util.function.BooleanSupplier", "BooleanSupplier", true, no_parents);
    REG_TYPE("java.util.function.ToIntFunction", "ToIntFunction", true, no_parents);
    REG_TYPE("java.util.function.ToLongFunction", "ToLongFunction", true, no_parents);
    REG_TYPE("java.util.function.ToDoubleFunction", "ToDoubleFunction", true, no_parents);

    REG_METHOD("java.util.function.Function", "apply", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.function.Function", "compose",
               pdxe_type_named(arena, "java.util.function.Function"));
    REG_METHOD("java.util.function.Function", "andThen",
               pdxe_type_named(arena, "java.util.function.Function"));
    REG_METHOD("java.util.function.Function", "identity",
               pdxe_type_named(arena, "java.util.function.Function"));

    REG_METHOD("java.util.function.BiFunction", "apply",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.function.BiFunction", "andThen",
               pdxe_type_named(arena, "java.util.function.BiFunction"));

    REG_METHOD("java.util.function.Predicate", "test", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.function.Predicate", "and",
               pdxe_type_named(arena, "java.util.function.Predicate"));
    REG_METHOD("java.util.function.Predicate", "or",
               pdxe_type_named(arena, "java.util.function.Predicate"));
    REG_METHOD("java.util.function.Predicate", "negate",
               pdxe_type_named(arena, "java.util.function.Predicate"));
    REG_METHOD("java.util.function.Predicate", "isEqual",
               pdxe_type_named(arena, "java.util.function.Predicate"));

    REG_METHOD("java.util.function.Consumer", "accept", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.function.Consumer", "andThen",
               pdxe_type_named(arena, "java.util.function.Consumer"));

    REG_METHOD("java.util.function.Supplier", "get", pdxe_type_named(arena, "java.lang.Object"));

    REG_METHOD("java.util.function.UnaryOperator", "identity",
               pdxe_type_named(arena, "java.util.function.UnaryOperator"));
    REG_METHOD("java.util.function.UnaryOperator", "apply",
               pdxe_type_named(arena, "java.lang.Object"));

    /* ── java.util.stream ────────────────────────────────────── */
    REG_TYPE("java.util.stream.Stream", "Stream", true, no_parents);
    REG_TYPE("java.util.stream.IntStream", "IntStream", true, no_parents);
    REG_TYPE("java.util.stream.LongStream", "LongStream", true, no_parents);
    REG_TYPE("java.util.stream.DoubleStream", "DoubleStream", true, no_parents);
    REG_TYPE("java.util.stream.Collectors", "Collectors", false, parents_object);
    REG_TYPE("java.util.stream.Collector", "Collector", true, no_parents);

    REG_METHOD("java.util.stream.Stream", "filter",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "map",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "flatMap",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "mapToInt",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.util.stream.Stream", "mapToLong",
               pdxe_type_named(arena, "java.util.stream.LongStream"));
    REG_METHOD("java.util.stream.Stream", "mapToDouble",
               pdxe_type_named(arena, "java.util.stream.DoubleStream"));
    REG_METHOD("java.util.stream.Stream", "sorted",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "distinct",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "limit",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "skip",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "peek",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "forEach", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.stream.Stream", "forEachOrdered", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.stream.Stream", "toArray",
               pdxe_type_slice(arena, pdxe_type_named(arena, "java.lang.Object")));
    REG_METHOD("java.util.stream.Stream", "toList", pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.stream.Stream", "reduce", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.stream.Stream", "collect", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.stream.Stream", "count", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.stream.Stream", "anyMatch", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.stream.Stream", "allMatch", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.stream.Stream", "noneMatch", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.stream.Stream", "findFirst",
               pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.stream.Stream", "findAny",
               pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.stream.Stream", "min", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.stream.Stream", "max", pdxe_type_named(arena, "java.util.Optional"));
    REG_METHOD("java.util.stream.Stream", "of",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "empty",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "concat",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "iterate",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.Stream", "generate",
               pdxe_type_named(arena, "java.util.stream.Stream"));

    REG_METHOD("java.util.stream.IntStream", "sum", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.stream.IntStream", "average",
               pdxe_type_named(arena, "java.util.OptionalDouble"));
    REG_METHOD("java.util.stream.IntStream", "max",
               pdxe_type_named(arena, "java.util.OptionalInt"));
    REG_METHOD("java.util.stream.IntStream", "min",
               pdxe_type_named(arena, "java.util.OptionalInt"));
    REG_METHOD("java.util.stream.IntStream", "count", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.stream.IntStream", "boxed",
               pdxe_type_named(arena, "java.util.stream.Stream"));
    REG_METHOD("java.util.stream.IntStream", "filter",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.util.stream.IntStream", "map",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.util.stream.IntStream", "range",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.util.stream.IntStream", "rangeClosed",
               pdxe_type_named(arena, "java.util.stream.IntStream"));
    REG_METHOD("java.util.stream.IntStream", "of",
               pdxe_type_named(arena, "java.util.stream.IntStream"));

    REG_METHOD("java.util.stream.Collectors", "toList",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "toSet",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "toMap",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "joining",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "groupingBy",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "partitioningBy",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "counting",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "summingInt",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "averagingDouble",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "mapping",
               pdxe_type_named(arena, "java.util.stream.Collector"));
    REG_METHOD("java.util.stream.Collectors", "reducing",
               pdxe_type_named(arena, "java.util.stream.Collector"));

    /* ── java.util.concurrent ────────────────────────────────── */
    REG_TYPE("java.util.concurrent.ExecutorService", "ExecutorService", true, no_parents);
    REG_TYPE("java.util.concurrent.Executors", "Executors", false, parents_object);
    REG_TYPE("java.util.concurrent.Future", "Future", true, no_parents);
    REG_TYPE("java.util.concurrent.CompletableFuture", "CompletableFuture", false,
             parents_completable_future);
    REG_TYPE("java.util.concurrent.ConcurrentHashMap", "ConcurrentHashMap", false,
             parents_concurrent_hashmap);
    REG_TYPE("java.util.concurrent.ConcurrentMap", "ConcurrentMap", true, parents_map);
    REG_TYPE("java.util.concurrent.TimeUnit", "TimeUnit", false, parents_object);
    REG_TYPE("java.util.concurrent.atomic.AtomicInteger", "AtomicInteger", false, parents_object);
    REG_TYPE("java.util.concurrent.atomic.AtomicLong", "AtomicLong", false, parents_object);
    REG_TYPE("java.util.concurrent.atomic.AtomicBoolean", "AtomicBoolean", false, parents_object);
    REG_TYPE("java.util.concurrent.atomic.AtomicReference", "AtomicReference", false,
             parents_object);
    REG_TYPE("java.util.concurrent.locks.Lock", "Lock", true, no_parents);
    REG_TYPE("java.util.concurrent.locks.ReentrantLock", "ReentrantLock", false,
             parents_reentrant_lock);
    REG_TYPE("java.util.concurrent.locks.ReadWriteLock", "ReadWriteLock", true, no_parents);

    REG_METHOD("java.util.concurrent.ExecutorService", "submit",
               pdxe_type_named(arena, "java.util.concurrent.Future"));
    REG_METHOD("java.util.concurrent.ExecutorService", "execute", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.ExecutorService", "shutdown", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.ExecutorService", "shutdownNow",
               pdxe_type_named(arena, "java.util.List"));
    REG_METHOD("java.util.concurrent.ExecutorService", "awaitTermination",
               pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.concurrent.ExecutorService", "isShutdown",
               pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.concurrent.ExecutorService", "isTerminated",
               pdxe_type_builtin(arena, "boolean"));

    REG_METHOD("java.util.concurrent.Executors", "newFixedThreadPool",
               pdxe_type_named(arena, "java.util.concurrent.ExecutorService"));
    REG_METHOD("java.util.concurrent.Executors", "newSingleThreadExecutor",
               pdxe_type_named(arena, "java.util.concurrent.ExecutorService"));
    REG_METHOD("java.util.concurrent.Executors", "newCachedThreadPool",
               pdxe_type_named(arena, "java.util.concurrent.ExecutorService"));
    REG_METHOD("java.util.concurrent.Executors", "newScheduledThreadPool",
               pdxe_type_named(arena, "java.util.concurrent.ExecutorService"));

    REG_METHOD("java.util.concurrent.Future", "get", pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.concurrent.Future", "isDone", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.concurrent.Future", "cancel", pdxe_type_builtin(arena, "boolean"));

    REG_METHOD("java.util.concurrent.CompletableFuture", "thenApply",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "thenAccept",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "thenCompose",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "thenCombine",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "exceptionally",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "join",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "supplyAsync",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "runAsync",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "completedFuture",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "allOf",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));
    REG_METHOD("java.util.concurrent.CompletableFuture", "anyOf",
               pdxe_type_named(arena, "java.util.concurrent.CompletableFuture"));

    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "get", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "set", pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "incrementAndGet",
               pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "decrementAndGet",
               pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "getAndIncrement",
               pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "compareAndSet",
               pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.util.concurrent.atomic.AtomicInteger", "addAndGet",
               pdxe_type_builtin(arena, "int"));
    REG_CTOR("java.util.concurrent.atomic.AtomicInteger", "AtomicInteger");

    REG_METHOD("java.util.concurrent.atomic.AtomicLong", "get",
               pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.util.concurrent.atomic.AtomicLong", "incrementAndGet",
               pdxe_type_builtin(arena, "long"));
    REG_CTOR("java.util.concurrent.atomic.AtomicLong", "AtomicLong");

    REG_METHOD("java.util.concurrent.atomic.AtomicReference", "get",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.concurrent.atomic.AtomicReference", "set",
               pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.atomic.AtomicReference", "compareAndSet",
               pdxe_type_builtin(arena, "boolean"));
    REG_CTOR("java.util.concurrent.atomic.AtomicReference", "AtomicReference");

    REG_METHOD("java.util.concurrent.locks.ReentrantLock", "lock",
               pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.locks.ReentrantLock", "unlock",
               pdxe_type_builtin(arena, "void"));
    REG_METHOD("java.util.concurrent.locks.ReentrantLock", "tryLock",
               pdxe_type_builtin(arena, "boolean"));
    REG_CTOR("java.util.concurrent.locks.ReentrantLock", "ReentrantLock");

    REG_METHOD("java.util.concurrent.ConcurrentHashMap", "put",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.concurrent.ConcurrentHashMap", "get",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_METHOD("java.util.concurrent.ConcurrentHashMap", "putIfAbsent",
               pdxe_type_named(arena, "java.lang.Object"));
    REG_CTOR("java.util.concurrent.ConcurrentHashMap", "ConcurrentHashMap");

    /* ── java.time ───────────────────────────────────────────── */
    REG_TYPE("java.time.LocalDate", "LocalDate", false, parents_object);
    REG_TYPE("java.time.LocalTime", "LocalTime", false, parents_object);
    REG_TYPE("java.time.LocalDateTime", "LocalDateTime", false, parents_object);
    REG_TYPE("java.time.ZonedDateTime", "ZonedDateTime", false, parents_object);
    REG_TYPE("java.time.OffsetDateTime", "OffsetDateTime", false, parents_object);
    REG_TYPE("java.time.Instant", "Instant", false, parents_object);
    REG_TYPE("java.time.Duration", "Duration", false, parents_object);
    REG_TYPE("java.time.Period", "Period", false, parents_object);
    REG_TYPE("java.time.ZoneId", "ZoneId", false, parents_object);
    REG_TYPE("java.time.format.DateTimeFormatter", "DateTimeFormatter", false, parents_object);

    REG_METHOD("java.time.LocalDate", "now", pdxe_type_named(arena, "java.time.LocalDate"));
    REG_METHOD("java.time.LocalDate", "of", pdxe_type_named(arena, "java.time.LocalDate"));
    REG_METHOD("java.time.LocalDate", "parse", pdxe_type_named(arena, "java.time.LocalDate"));
    REG_METHOD("java.time.LocalDate", "plusDays", pdxe_type_named(arena, "java.time.LocalDate"));
    REG_METHOD("java.time.LocalDate", "minusDays", pdxe_type_named(arena, "java.time.LocalDate"));
    REG_METHOD("java.time.LocalDate", "getYear", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.time.LocalDate", "getMonth", pdxe_type_named(arena, "java.time.Month"));
    REG_METHOD("java.time.LocalDate", "getDayOfMonth", pdxe_type_builtin(arena, "int"));
    REG_METHOD("java.time.LocalDate", "getDayOfWeek",
               pdxe_type_named(arena, "java.time.DayOfWeek"));
    REG_METHOD("java.time.LocalDate", "isAfter", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.time.LocalDate", "isBefore", pdxe_type_builtin(arena, "boolean"));
    REG_METHOD("java.time.LocalDate", "format", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.time.LocalDate", "toString", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.time.LocalDateTime", "now",
               pdxe_type_named(arena, "java.time.LocalDateTime"));
    REG_METHOD("java.time.LocalDateTime", "of",
               pdxe_type_named(arena, "java.time.LocalDateTime"));
    REG_METHOD("java.time.LocalDateTime", "parse",
               pdxe_type_named(arena, "java.time.LocalDateTime"));
    REG_METHOD("java.time.LocalDateTime", "plusHours",
               pdxe_type_named(arena, "java.time.LocalDateTime"));
    REG_METHOD("java.time.LocalDateTime", "minusHours",
               pdxe_type_named(arena, "java.time.LocalDateTime"));
    REG_METHOD("java.time.LocalDateTime", "format", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.time.LocalDateTime", "toString", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.time.Instant", "now", pdxe_type_named(arena, "java.time.Instant"));
    REG_METHOD("java.time.Instant", "ofEpochMilli", pdxe_type_named(arena, "java.time.Instant"));
    REG_METHOD("java.time.Instant", "ofEpochSecond", pdxe_type_named(arena, "java.time.Instant"));
    REG_METHOD("java.time.Instant", "toEpochMilli", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.time.Instant", "getEpochSecond", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.time.Instant", "plus", pdxe_type_named(arena, "java.time.Instant"));
    REG_METHOD("java.time.Instant", "minus", pdxe_type_named(arena, "java.time.Instant"));

    REG_METHOD("java.time.Duration", "ofSeconds", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "ofMillis", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "ofMinutes", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "ofHours", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "ofDays", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "between", pdxe_type_named(arena, "java.time.Duration"));
    REG_METHOD("java.time.Duration", "toMillis", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.time.Duration", "toSeconds", pdxe_type_builtin(arena, "long"));
    REG_METHOD("java.time.Duration", "toMinutes", pdxe_type_builtin(arena, "long"));

    REG_METHOD("java.time.ZoneId", "of", pdxe_type_named(arena, "java.time.ZoneId"));
    REG_METHOD("java.time.ZoneId", "systemDefault", pdxe_type_named(arena, "java.time.ZoneId"));
    REG_METHOD("java.time.ZoneId", "getId", pdxe_type_named(arena, "java.lang.String"));

    REG_METHOD("java.time.format.DateTimeFormatter", "ofPattern",
               pdxe_type_named(arena, "java.time.format.DateTimeFormatter"));
    REG_METHOD("java.time.format.DateTimeFormatter", "format",
               pdxe_type_named(arena, "java.lang.String"));

    /* ── java.net (minimal) ──────────────────────────────────── */
    REG_TYPE("java.net.URI", "URI", false, parents_object);
    REG_TYPE("java.net.URL", "URL", false, parents_object);
    REG_METHOD("java.net.URI", "create", pdxe_type_named(arena, "java.net.URI"));
    REG_METHOD("java.net.URI", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_METHOD("java.net.URI", "toURL", pdxe_type_named(arena, "java.net.URL"));
    REG_METHOD("java.net.URL", "openStream", pdxe_type_named(arena, "java.io.InputStream"));
    REG_METHOD("java.net.URL", "toString", pdxe_type_named(arena, "java.lang.String"));
    REG_CTOR("java.net.URL", "URL");
    REG_CTOR("java.net.URI", "URI");
}
