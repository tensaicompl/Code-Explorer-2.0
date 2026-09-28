namespace Acme.Core
{
    public class Repo
    {
        public string Find(int id) { return "x"; }
        public static Repo Create() { return new Repo(); }
    }
}
