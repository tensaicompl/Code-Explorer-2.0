using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Corpus.Orders
{
    /// <summary>Records, generics with constraints, LINQ, async and attributes.</summary>
    public record Line(string Sku, int Qty, decimal Price)
    {
        public decimal Total => Qty * Price;
    }

    public interface IRepository<TKey, TValue> where TValue : class
    {
        TValue? Find(TKey key);
        void Save(TKey key, TValue value);
    }

    [Serializable]
    public sealed class Order
    {
        public string Reference { get; init; } = "";
        public List<Line> Lines { get; } = new();
        public event EventHandler<Line>? LineAdded;

        public Order Add(string sku, int qty = 1, decimal price = 0m)
        {
            if (qty <= 0) throw new ArgumentOutOfRangeException(nameof(qty), $"bad quantity {qty}");
            var line = new Line(sku, qty, price);
            Lines.Add(line);
            LineAdded?.Invoke(this, line);
            return this;
        }

        public string Describe() => Lines switch
        {
            { Count: 0 } => "empty",
            [var only] => $"one line: {only.Sku}",
            _ => $"{Lines.Count} lines, total {Lines.Sum(l => l.Total):0.00}",
        };
    }

    public class MemoryRepository<TKey, TValue> : IRepository<TKey, TValue>
        where TKey : notnull
        where TValue : class
    {
        private readonly Dictionary<TKey, TValue> _items = new();

        public TValue? Find(TKey key) => _items.TryGetValue(key, out var value) ? value : null;

        public void Save(TKey key, TValue value) => _items[key] = value;
    }

    public static class Reports
    {
        public static async Task<IReadOnlyList<string>> SummariseAsync(IEnumerable<Order> orders)
        {
            await Task.Yield();
            var query =
                from o in orders
                where o.Lines.Count > 0
                orderby o.Reference descending
                select $"{o.Reference}: {o.Describe()}";
            return query.ToList();
        }

        public static string Verbatim => @"C:\orders\""quoted"" — résumé";

        public static string Raw => """
            {"raw": "json", "nested": {"ok": true}}
            """;
    }
}
