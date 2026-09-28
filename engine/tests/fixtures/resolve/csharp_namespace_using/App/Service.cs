using Acme.Core;

namespace Acme.App
{
    public class Service
    {
        public string Run()
        {
            Repo r = Repo.Create();
            var s = new Repo();
            return r.Find(1) + s.Find(2);
        }
    }
}
