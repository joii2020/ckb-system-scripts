# Build all version
make -f Makefile.clang clean
CLANG_VERSION=16
make -f Makefile.clang all CC=clang-$CLANG_VERSION LD=ld.lld-$CLANG_VERSION OBJCOPY=llvm-objcopy-$CLANG_VERSION AR=llvm-ar-$CLANG_VERSION
cp specs/cells/secp256k1_blake160_sighash_all specs/cells/secp256k1_blake160_sighash_all_llvm_$CLANG_VERSION
clang-$CLANG_VERSION --version

make -f Makefile.clang clean
CLANG_VERSION=17
make -f Makefile.clang all CC=clang-$CLANG_VERSION LD=ld.lld-$CLANG_VERSION OBJCOPY=llvm-objcopy-$CLANG_VERSION AR=llvm-ar-$CLANG_VERSION
cp specs/cells/secp256k1_blake160_sighash_all specs/cells/secp256k1_blake160_sighash_all_llvm_$CLANG_VERSION
clang-$CLANG_VERSION --version

make -f Makefile.clang clean
CLANG_VERSION=18
make -f Makefile.clang all CC=clang-$CLANG_VERSION LD=ld.lld-$CLANG_VERSION OBJCOPY=llvm-objcopy-$CLANG_VERSION AR=llvm-ar-$CLANG_VERSION
cp specs/cells/secp256k1_blake160_sighash_all specs/cells/secp256k1_blake160_sighash_all_llvm_$CLANG_VERSION
clang-$CLANG_VERSION --version



cargo test --features="test_llvm_version" --lib -- tests::secp256k1_blake160_sighash_all::test_sighash_benchmark --exact --show-output --nocapture