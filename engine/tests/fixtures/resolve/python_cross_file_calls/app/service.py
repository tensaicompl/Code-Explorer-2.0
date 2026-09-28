from app.models import make_user, User

def run():
    u = make_user()
    v = User()
    print(len('x'))
    return u.display_name(), v.display_name()

def apply(f):
    return f()

def go():
    return apply(make_user)
