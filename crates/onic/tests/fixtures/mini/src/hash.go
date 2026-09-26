package hash

import enc "src/encode"

func HashPassword() {
	EncodePassword()
	func() {
		EncodePassword()
	}()
	hasher.Digest()
}



func (h *PasswordHasher) Digest() {}

type Hasher interface { Digest(string) string }

type PasswordHasher struct { Salt }

func init() {}

func DeferEncode() {
	defer EncodePassword()
}

func viaAlias() {
	f := HashPassword
	f()
	g := func() {}
	g()
}

func viaMethodValue() {
	digest := hasher.Digest
	digest()
}

var digest = HashPassword

func viaNamedResult() (digest func()) {
	digest()
	return
}

func viaPkgVar() {
	digest()
}

type Hash string

func viaConv() {
	_ = Hash("")
}
