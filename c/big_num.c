

#include <stdint.h>

#define CHAR_BIT 8

//===-- int_lib.h - configuration header for compiler-rt  -----------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file is not part of the interface of this library.
//
// This file defines various standard types, most importantly a number of unions
// used to access parts of larger types.
//
//===----------------------------------------------------------------------===//

typedef int ti_int __attribute__((mode(TI)));
typedef unsigned tu_int __attribute__((mode(TI)));
typedef uint64_t du_int;
typedef int64_t di_int;
typedef int32_t si_int;

typedef union {
  ti_int all;
  struct {
// #if _YUGA_LITTLE_ENDIAN
    du_int low;
    di_int high;
// #else
//     di_int high;
//     du_int low;
// #endif // _YUGA_LITTLE_ENDIAN
  } s;
} twords;


typedef union {
  tu_int all;
  struct {
// #if _YUGA_LITTLE_ENDIAN
    du_int low;
    du_int high;
// #else
//     du_int high;
//     du_int low;
// #endif // _YUGA_LITTLE_ENDIAN
  } s;
} utwords;

//===-- udivmodti4.c - Implement __udivmodti4 -----------------------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file implements __udivmodti4 for the compiler_rt library.
//
//===----------------------------------------------------------------------===//

// #include "int_lib.h"

// #ifdef CRT_HAS_128BIT

// Returns the 128 bit division result by 64 bit. Result must fit in 64 bits.
// Remainder stored in r.
// Taken and adjusted from libdivide libdivide_128_div_64_to_64 division
// fallback. For a correctness proof see the reference for this algorithm
// in Knuth, Volume 2, section 4.3.1, Algorithm D.
// UNUSED
static inline du_int udiv128by64to64default(du_int u1, du_int u0, du_int v,
                                            du_int *r) {
  const unsigned n_udword_bits = sizeof(du_int) * CHAR_BIT;
  const du_int b = (1ULL << (n_udword_bits / 2)); // Number base (32 bits)
  du_int un1, un0;                                // Norm. dividend LSD's
  du_int vn1, vn0;                                // Norm. divisor digits
  du_int q1, q0;                                  // Quotient digits
  du_int un64, un21, un10;                        // Dividend digit pairs
  du_int rhat;                                    // A remainder
  si_int s;                                       // Shift amount for normalization

  s = __builtin_clzll(v);
  if (s > 0) {
    // Normalize the divisor.
    v = v << s;
    un64 = (u1 << s) | (u0 >> (n_udword_bits - s));
    un10 = u0 << s; // Shift dividend left
  } else {
    // Avoid undefined behavior of (u0 >> 64).
    un64 = u1;
    un10 = u0;
  }

  // Break divisor up into two 32-bit digits.
  vn1 = v >> (n_udword_bits / 2);
  vn0 = v & 0xFFFFFFFF;

  // Break right half of dividend into two digits.
  un1 = un10 >> (n_udword_bits / 2);
  un0 = un10 & 0xFFFFFFFF;

  // Compute the first quotient digit, q1.
  q1 = un64 / vn1;
  rhat = un64 - q1 * vn1;

  // q1 has at most error 2. No more than 2 iterations.
  while (q1 >= b || q1 * vn0 > b * rhat + un1) {
    q1 = q1 - 1;
    rhat = rhat + vn1;
    if (rhat >= b)
      break;
  }

  un21 = un64 * b + un1 - q1 * v;

  // Compute the second quotient digit.
  q0 = un21 / vn1;
  rhat = un21 - q0 * vn1;

  // q0 has at most error 2. No more than 2 iterations.
  while (q0 >= b || q0 * vn0 > b * rhat + un0) {
    q0 = q0 - 1;
    rhat = rhat + vn1;
    if (rhat >= b)
      break;
  }

  *r = (un21 * b + un0 - q0 * v) >> s;
  return q1 * b + q0;
}

static inline du_int udiv128by64to64(du_int u1, du_int u0, du_int v,
                                     du_int *r) {
#if defined(__x86_64__)
  du_int result;
  __asm__("divq %[v]"
          : "=a"(result), "=d"(*r)
          : [ v ] "r"(v), "a"(u0), "d"(u1));
  return result;
#else
  return udiv128by64to64default(u1, u0, v, r);
#endif
}

// Effects: if rem != 0, *rem = a % b
// Returns: a / b

// COMPILER_RT_ABI
tu_int __udivmodti4(tu_int a, tu_int b, tu_int *rem) {
  const unsigned n_utword_bits = sizeof(tu_int) * CHAR_BIT;
  utwords dividend;
  dividend.all = a;
  utwords divisor;
  divisor.all = b;
  utwords quotient;
  utwords remainder;
  if (divisor.all > dividend.all) {
    if (rem)
      *rem = dividend.all;
    return 0;
  }
  // When the divisor fits in 64 bits, we can use an optimized path.
  if (divisor.s.high == 0) {
    remainder.s.high = 0;
    if (dividend.s.high < divisor.s.low) {
      // The result fits in 64 bits.
      quotient.s.low = udiv128by64to64(dividend.s.high, dividend.s.low,
                                       divisor.s.low, &remainder.s.low);
      quotient.s.high = 0;
    } else {
      // First, divide with the high part to get the remainder in dividend.s.high.
      // After that dividend.s.high < divisor.s.low.
      quotient.s.high = dividend.s.high / divisor.s.low;
      dividend.s.high = dividend.s.high % divisor.s.low;
      quotient.s.low = udiv128by64to64(dividend.s.high, dividend.s.low,
                                       divisor.s.low, &remainder.s.low);
    }
    if (rem)
      *rem = remainder.all;
    return quotient.all;
  }
  // 0 <= shift <= 63.
  si_int shift =
      __builtin_clzll(divisor.s.high) - __builtin_clzll(dividend.s.high);
  divisor.all <<= shift;
  quotient.s.high = 0;
  quotient.s.low = 0;
  for (; shift >= 0; --shift) {
    quotient.s.low <<= 1;
    // Branch free version of.
    // if (dividend.all >= divisor.all)
    // {
    //    dividend.all -= divisor.all;
    //    carry = 1;
    // }
    const ti_int s =
        (ti_int)(divisor.all - dividend.all - 1) >> (n_utword_bits - 1);
    quotient.s.low |= s & 1;
    dividend.all -= divisor.all & s;
    divisor.all >>= 1;
  }
  if (rem)
    *rem = dividend.all;
  return quotient.all;
}

// #endif // CRT_HAS_128BIT



//===-- udivti3.c - Implement __udivti3 -----------------------------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file implements __udivti3 for the compiler_rt library.
//
//===----------------------------------------------------------------------===//

// COMPILER_RT_ABI
tu_int __udivti3(tu_int a, tu_int b) {
  return __udivmodti4(a, b, 0);
}



//===-- int_div_impl.inc - Integer division ---------------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Helpers used by __udivsi3, __umodsi3, __udivdi3, and __umodsi3.
//
//===----------------------------------------------------------------------===//

#define COMPUTE_UDIV(a, b) __udivmodti4((a), (b), (tu_int *)0)

static __inline ti_int __divXi3(ti_int a, ti_int b) {
  const int N = (int)(sizeof(ti_int) * CHAR_BIT) - 1;
  ti_int s_a = a >> N;                            // s_a = a < 0 ? -1 : 0
  ti_int s_b = b >> N;                            // s_b = b < 0 ? -1 : 0
  tu_int a_u = (tu_int)(a ^ s_a) + (-s_a);    // negate if s_a == -1
  tu_int b_u = (tu_int)(b ^ s_b) + (-s_b);    // negate if s_b == -1
  s_a ^= s_b;                                       // sign of quotient
  return (COMPUTE_UDIV(a_u, b_u) ^ s_a) + (-s_a);   // negate if s_a == -1
}



//===-- divti3.c - Implement __divti3 -------------------------------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file implements __divti3 for the compiler_rt library.
//
//===----------------------------------------------------------------------===//

// COMPILER_RT_ABI
ti_int __divti3(ti_int a, ti_int b) { return __divXi3(a, b); }

static ti_int __mulddi3(du_int a, du_int b) {
  twords r;
  const int bits_in_dword_2 = (int)(sizeof(di_int) * CHAR_BIT) / 2;
  const du_int lower_mask = (du_int)~0 >> bits_in_dword_2;
  r.s.low = (a & lower_mask) * (b & lower_mask);
  du_int t = r.s.low >> bits_in_dword_2;
  r.s.low &= lower_mask;
  t += (a >> bits_in_dword_2) * (b & lower_mask);
  r.s.low += (t & lower_mask) << bits_in_dword_2;
  r.s.high = t >> bits_in_dword_2;
  t = r.s.low >> bits_in_dword_2;
  r.s.low &= lower_mask;
  t += (b >> bits_in_dword_2) * (a & lower_mask);
  r.s.low += (t & lower_mask) << bits_in_dword_2;
  r.s.high += t >> bits_in_dword_2;
  r.s.high += (a >> bits_in_dword_2) * (b >> bits_in_dword_2);
  return r.all;
}



//===-- multi3.c - Implement __multi3 -------------------------------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file implements __multi3 for the compiler_rt library.
//
//===----------------------------------------------------------------------===//

// Returns: a * b

// COMPILER_RT_ABI
ti_int __multi3(ti_int a, ti_int b) {
  twords x;
  x.all = a;
  twords y;
  y.all = b;
  twords r;
  r.all = __mulddi3(x.s.low, y.s.low);
  r.s.high += x.s.high * y.s.low + x.s.low * y.s.high;
  return r.all;
}
