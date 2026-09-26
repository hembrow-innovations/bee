def encode_password(password):
    return password


def hash_password(password):
    return encode_password(password)


class PasswordHasher:
    class Salt:
        def stamp(self):
            return lambda: None

    def digest(self, password):
        return hash_password(password)

    def __call__(self, password):
        return hash_password(password)


def run_digest(password):
    hasher = PasswordHasher()
    return hasher.digest(password)


def run_encode(password):
    return (encode_password)(password)


def run_getattr(password):
    hasher = PasswordHasher()
    return getattr(hasher, "digest")(password)  # noqa: B009


def run_subscript(password):
    hasher = PasswordHasher()
    return hasher["digest"](password)


def run_call(password):
    return PasswordHasher()(password)


def run_instance(password):
    hasher = PasswordHasher()
    return hasher(password)


def run_nested(password):
    hasher = PasswordHasher()

    def nested_instance():
        return hasher(password)

    return nested_instance()


def run_lambda(password):
    hasher = PasswordHasher()
    return lambda: hasher(password)


def run_alias(password):
    hasher = PasswordHasher()
    alias = hasher
    return alias(password)


hasher = PasswordHasher()


def run_module(password):
    return hasher(password)


hasher("secret")


lambda: hasher("secret")


def run_module_nested(password):
    def nested_module():
        return hasher(password)

    return nested_module()


def run_module_lambda(password):
    return lambda: hasher(password)


def run_before(password):
    return hasher2(password)


hasher2 = PasswordHasher()


def run_clear(password):
    hasher = PasswordHasher()

    def nested_clear():
        hasher = None
        return hasher(password)

    return hasher(password)


def run_add(password):
    def nested_add():
        hasher3 = PasswordHasher()
        return hasher3(password)

    return hasher3(password)


def run_set_clear(password):
    hasher = None
    return hasher(password)


def run_set_keep(password):
    return hasher(password)


def run_set_add(password):
    hasher4 = PasswordHasher()
    return hasher4(password)


def run_set_miss(password):
    return hasher4(password)


def run_attr_ctor(password):
    salt = PasswordHasher.Salt()
    return salt(password)


def run_param(hasher):
    return hasher(password)


def run_param_keep(password):
    return hasher(password)


lambda hasher: hasher(password)


def run_lambda_param_keep(password):
    return hasher(password)


lambda: (hasher := None) or hasher(password)


def run_lambda_clear_keep(password):
    return hasher(password)


lambda: (hasher5 := PasswordHasher()) and hasher5(password)


def run_lambda_add_miss(password):
    return hasher5(password)


lambda: (hasher6 := hasher) and hasher6(password)


def run_lambda_alias_miss(password):
    return hasher6(password)


(hasher7 := PasswordHasher())


def run_walrus(password):
    return hasher7(password)


(hasher8 := hasher)


def run_walrus_alias(password):
    return hasher8(password)


(hasher8 := None)


def run_walrus_clear(password):
    return hasher8(password)


def run_fn_walrus(password):
    (hasher9 := PasswordHasher())
    return hasher9(password)


def run_fn_walrus_miss(password):
    return hasher9(password)


def run_fn_walrus_alias(password):
    (hasher10 := hasher)
    return hasher10(password)


def run_fn_walrus_alias_miss(password):
    return hasher10(password)


def run_fn_walrus_clear(password):
    (hasher := None)
    return hasher(password)


def run_fn_walrus_clear_keep(password):
    return hasher(password)


class HasherHolder:
    hasher11 = PasswordHasher()

    def run_class_inherit(self, password):
        return hasher11(password)

    hasher11("secret")

    lambda: hasher11("secret")


def run_class_inherit_miss(password):
    return hasher11(password)


class AttrLhsHolder:
    def run_attr_lhs(self, password):
        self.hasher12 = PasswordHasher()
        return self.hasher12(password)


def run_attr_lhs_miss(password):
    return hasher12(password)


class ClassWalrusHolder:
    (hasher13 := PasswordHasher())
    hasher13("secret")


def run_class_walrus_miss(password):
    return hasher13(password)


class ClassWalrusAliasHolder:
    hasher14 = PasswordHasher()
    (hasher15 := hasher14)
    hasher15("secret")


def run_class_walrus_alias_miss(password):
    return hasher15(password)


class ClassWalrusClearHolder:
    hasher16 = PasswordHasher()
    (hasher16 := None)
    hasher16("secret")


def run_class_walrus_clear_keep(password):
    return hasher(password)


class ClassBeforeAssignHolder:
    def run_class_before(self, password):
        return hasher17(password)

    hasher17 = PasswordHasher()


class ClassAssignClearHolder:
    hasher18 = PasswordHasher()
    hasher18 = None
    hasher18("secret")


def run_class_assign_clear_keep(password):
    return hasher(password)


class ClassAssignAliasHolder:
    hasher19 = PasswordHasher()
    hasher20 = hasher19
    hasher20("secret")


def run_class_assign_alias_miss(password):
    return hasher20(password)
