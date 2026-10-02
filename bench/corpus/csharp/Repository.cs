namespace Corpus.Storage;

public abstract class Entity<TId> where TId : struct
{
    public TId Id { get; protected set; }
    public override string ToString() => $"{GetType().Name}#{Id}";
}

public class Customer : Entity<int>
{
    public Customer(int id, string name)
    {
        Id = id;
        Name = name;
    }

    public string Name { get; }

    public static bool operator ==(Customer? a, Customer? b) => a?.Id == b?.Id;
    public static bool operator !=(Customer? a, Customer? b) => !(a == b);
    public override bool Equals(object? obj) => obj is Customer c && c.Id == Id;
    public override int GetHashCode() => Id.GetHashCode();
}

public static class Extensions
{
    public static IEnumerable<T> EveryOther<T>(this IEnumerable<T> source)
    {
        var i = 0;
        foreach (var item in source)
        {
            if (i++ % 2 == 0) yield return item;
        }
    }
}
