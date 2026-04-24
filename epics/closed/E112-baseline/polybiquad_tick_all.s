0000000000000b4c <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17hd60283cfe77dfb42E>:
     b4c: ad400400     	ldp	q0, q1, [x0]
     b50: ad401427     	ldp	q7, q5, [x1]
     b54: 6e27dc00     	fmul.4s	v0, v0, v7
     b58: ad540c02     	ldp	q2, q3, [x0, #0x280]
     b5c: 4e22d400     	fadd.4s	v0, v0, v2
     b60: 6e25dc21     	fmul.4s	v1, v1, v5
     b64: 4e23d421     	fadd.4s	v1, v1, v3
     b68: ad410c02     	ldp	q2, q3, [x0, #0x20]
     b6c: ad411026     	ldp	q6, q4, [x1, #0x20]
     b70: 6e26dc42     	fmul.4s	v2, v2, v6
     b74: ad554410     	ldp	q16, q17, [x0, #0x2a0]
     b78: 4e30d442     	fadd.4s	v2, v2, v16
     b7c: 6e24dc63     	fmul.4s	v3, v3, v4
     b80: 4e31d463     	fadd.4s	v3, v3, v17
     b84: 4ea01c10     	mov.16b	v16, v0
     b88: 4ea11c31     	mov.16b	v17, v1
     b8c: 4ea21c52     	mov.16b	v18, v2
     b90: 4ea31c73     	mov.16b	v19, v3
     b94: 34000cc2     	cbz	w2, 0xd2c <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17hd60283cfe77dfb42E+0x1e0>
     b98: 6dbe2beb     	stp	d11, d10, [sp, #-0x20]!
     b9c: 6d0123e9     	stp	d9, d8, [sp, #0x10]
     ba0: 4f00f493     	fmov.4s	v19, #2.50000000
     ba4: 6e33e410     	fcmge.4s	v16, v0, v19
     ba8: 4f04f495     	fmov.4s	v21, #-2.50000000
     bac: 6e20e6b1     	fcmge.4s	v17, v21, v0
     bb0: 6e20dc12     	fmul.4s	v18, v0, v0
     bb4: 6e32de5a     	fmul.4s	v26, v18, v18
     bb8: 6e3ade5b     	fmul.4s	v27, v18, v26
     bbc: 52900009     	mov	w9, #0x8000             ; =32768
     bc0: 72a893a9     	movk	w9, #0x449d, lsl #16
     bc4: 4e040d36     	dup.4s	v22, w9
     bc8: 6e36de57     	fmul.4s	v23, v18, v22
     bcc: 528d8009     	mov	w9, #0x6c00             ; =27648
     bd0: 72a8c449     	movk	w9, #0x4622, lsl #16
     bd4: 4e040d34     	dup.4s	v20, w9
     bd8: 4e34d6f8     	fadd.4s	v24, v23, v20
     bdc: 4f01f6b7     	fmov.4s	v23, #21.00000000
     be0: 6e37df59     	fmul.4s	v25, v26, v23
     be4: 4e39d718     	fadd.4s	v24, v24, v25
     be8: 6e38dc1c     	fmul.4s	v28, v0, v24
     bec: 52950009     	mov	w9, #0xa800             ; =43008
     bf0: 72a8b269     	movk	w9, #0x4593, lsl #16
     bf4: 4e040d38     	dup.4s	v24, w9
     bf8: 6e38de52     	fmul.4s	v18, v18, v24
     bfc: 52a86a49     	mov	w9, #0x43520000         ; =1129447424
     c00: 4e040d39     	dup.4s	v25, w9
     c04: 4e34d652     	fadd.4s	v18, v18, v20
     c08: 6e39df5a     	fmul.4s	v26, v26, v25
     c0c: 4e3ad652     	fadd.4s	v18, v18, v26
     c10: 4f00f61a     	fmov.4s	v26, #4.00000000
     c14: 6e3adf7b     	fmul.4s	v27, v27, v26
     c18: 4e3bd652     	fadd.4s	v18, v18, v27
     c1c: 6e32ff92     	fdiv.4s	v18, v28, v18
     c20: 4e701e31     	bic.16b	v17, v17, v16
     c24: 4f07f61b     	fmov.4s	v27, #-1.00000000
     c28: 6e721f71     	bsl.16b	v17, v27, v18
     c2c: 4f03f61c     	fmov.4s	v28, #1.00000000
     c30: 6e711f90     	bsl.16b	v16, v28, v17
     c34: 6e33e431     	fcmge.4s	v17, v1, v19
     c38: 6e21e6b2     	fcmge.4s	v18, v21, v1
     c3c: 6e21dc3d     	fmul.4s	v29, v1, v1
     c40: 6e3ddfbe     	fmul.4s	v30, v29, v29
     c44: 6e3edfbf     	fmul.4s	v31, v29, v30
     c48: 6e36dfa8     	fmul.4s	v8, v29, v22
     c4c: 4e34d508     	fadd.4s	v8, v8, v20
     c50: 6e37dfc9     	fmul.4s	v9, v30, v23
     c54: 4e29d508     	fadd.4s	v8, v8, v9
     c58: 6e28dc28     	fmul.4s	v8, v1, v8
     c5c: 6e38dfbd     	fmul.4s	v29, v29, v24
     c60: 4e34d7bd     	fadd.4s	v29, v29, v20
     c64: 6e39dfde     	fmul.4s	v30, v30, v25
     c68: 4e3ed7bd     	fadd.4s	v29, v29, v30
     c6c: 6e3adffe     	fmul.4s	v30, v31, v26
     c70: 4e3ed7bd     	fadd.4s	v29, v29, v30
     c74: 6e3dfd1d     	fdiv.4s	v29, v8, v29
     c78: 4e711e52     	bic.16b	v18, v18, v17
     c7c: 6e7d1f72     	bsl.16b	v18, v27, v29
     c80: 6e721f91     	bsl.16b	v17, v28, v18
     c84: 6e33e452     	fcmge.4s	v18, v2, v19
     c88: 6e22e6bd     	fcmge.4s	v29, v21, v2
     c8c: 6e22dc5e     	fmul.4s	v30, v2, v2
     c90: 6e3edfdf     	fmul.4s	v31, v30, v30
     c94: 6e3fdfc8     	fmul.4s	v8, v30, v31
     c98: 6e36dfc9     	fmul.4s	v9, v30, v22
     c9c: 4e34d529     	fadd.4s	v9, v9, v20
     ca0: 6e37dfea     	fmul.4s	v10, v31, v23
     ca4: 4e2ad529     	fadd.4s	v9, v9, v10
     ca8: 6e29dc49     	fmul.4s	v9, v2, v9
     cac: 6e38dfde     	fmul.4s	v30, v30, v24
     cb0: 4e34d7de     	fadd.4s	v30, v30, v20
     cb4: 6e39dfff     	fmul.4s	v31, v31, v25
     cb8: 4e3fd7de     	fadd.4s	v30, v30, v31
     cbc: 6e3add1f     	fmul.4s	v31, v8, v26
     cc0: 4e3fd7de     	fadd.4s	v30, v30, v31
     cc4: 6e3efd3e     	fdiv.4s	v30, v9, v30
     cc8: 4e721fbd     	bic.16b	v29, v29, v18
     ccc: 6e7e1f7d     	bsl.16b	v29, v27, v30
     cd0: 6e7d1f92     	bsl.16b	v18, v28, v29
     cd4: 6e33e473     	fcmge.4s	v19, v3, v19
     cd8: 6e23e6b5     	fcmge.4s	v21, v21, v3
     cdc: 6e23dc7d     	fmul.4s	v29, v3, v3
     ce0: 6e3ddfbe     	fmul.4s	v30, v29, v29
     ce4: 6e3edfbf     	fmul.4s	v31, v29, v30
     ce8: 6e36dfb6     	fmul.4s	v22, v29, v22
     cec: 4e34d6d6     	fadd.4s	v22, v22, v20
     cf0: 6e37dfd7     	fmul.4s	v23, v30, v23
     cf4: 4e37d6d6     	fadd.4s	v22, v22, v23
     cf8: 6e36dc76     	fmul.4s	v22, v3, v22
     cfc: 6e38dfb7     	fmul.4s	v23, v29, v24
     d00: 4e34d6f4     	fadd.4s	v20, v23, v20
     d04: 6e39dfd7     	fmul.4s	v23, v30, v25
     d08: 4e37d694     	fadd.4s	v20, v20, v23
     d0c: 6e3adff7     	fmul.4s	v23, v31, v26
     d10: 4e37d694     	fadd.4s	v20, v20, v23
     d14: 6e34fed4     	fdiv.4s	v20, v22, v20
     d18: 4e731eb5     	bic.16b	v21, v21, v19
     d1c: 6eb51f74     	bit.16b	v20, v27, v21
     d20: 6e741f93     	bsl.16b	v19, v28, v20
     d24: 6d4123e9     	ldp	d9, d8, [sp, #0x10]
     d28: 6cc22beb     	ldp	d11, d10, [sp], #0x20
     d2c: ad425414     	ldp	q20, q21, [x0, #0x40]
     d30: 6e34dcf4     	fmul.4s	v20, v7, v20
     d34: ad465c16     	ldp	q22, q23, [x0, #0xc0]
     d38: 6e36de16     	fmul.4s	v22, v16, v22
     d3c: 4eb6d694     	fsub.4s	v20, v20, v22
     d40: ad566016     	ldp	q22, q24, [x0, #0x2c0]
     d44: 4e34d6d4     	fadd.4s	v20, v22, v20
     d48: 6e35dcb5     	fmul.4s	v21, v5, v21
     d4c: 6e37de36     	fmul.4s	v22, v17, v23
     d50: 4eb6d6b5     	fsub.4s	v21, v21, v22
     d54: 4e35d715     	fadd.4s	v21, v24, v21
     d58: ad145414     	stp	q20, q21, [x0, #0x280]
     d5c: ad435414     	ldp	q20, q21, [x0, #0x60]
     d60: 6e34dcd4     	fmul.4s	v20, v6, v20
     d64: ad475c16     	ldp	q22, q23, [x0, #0xe0]
     d68: 6e36de56     	fmul.4s	v22, v18, v22
     d6c: 4eb6d694     	fsub.4s	v20, v20, v22
     d70: ad576016     	ldp	q22, q24, [x0, #0x2e0]
     d74: 4e34d6d4     	fadd.4s	v20, v22, v20
     d78: 6e35dc95     	fmul.4s	v21, v4, v21
     d7c: 6e37de76     	fmul.4s	v22, v19, v23
     d80: 4eb6d6b5     	fsub.4s	v21, v21, v22
     d84: 4e35d715     	fadd.4s	v21, v24, v21
     d88: ad155414     	stp	q20, q21, [x0, #0x2a0]
     d8c: ad445414     	ldp	q20, q21, [x0, #0x80]
     d90: 6e34dce7     	fmul.4s	v7, v7, v20
     d94: ad485814     	ldp	q20, q22, [x0, #0x100]
     d98: 6e34de10     	fmul.4s	v16, v16, v20
     d9c: 4eb0d4e7     	fsub.4s	v7, v7, v16
     da0: 6e35dca5     	fmul.4s	v5, v5, v21
     da4: 6e36de30     	fmul.4s	v16, v17, v22
     da8: 4eb0d4a5     	fsub.4s	v5, v5, v16
     dac: ad161407     	stp	q7, q5, [x0, #0x2c0]
     db0: ad451c05     	ldp	q5, q7, [x0, #0xa0]
     db4: 6e25dcc5     	fmul.4s	v5, v6, v5
     db8: ad494006     	ldp	q6, q16, [x0, #0x120]
     dbc: 6e26de46     	fmul.4s	v6, v18, v6
     dc0: 4ea6d4a5     	fsub.4s	v5, v5, v6
     dc4: 6e27dc84     	fmul.4s	v4, v4, v7
     dc8: 6e30de66     	fmul.4s	v6, v19, v16
     dcc: 4ea6d484     	fsub.4s	v4, v4, v6
     dd0: ad171005     	stp	q5, q4, [x0, #0x2e0]
     dd4: 34000663     	cbz	w3, 0xea0 <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17hd60283cfe77dfb42E+0x354>
     dd8: ad4a1404     	ldp	q4, q5, [x0, #0x140]
     ddc: ad401c06     	ldp	q6, q7, [x0]
     de0: 4e26d484     	fadd.4s	v4, v4, v6
     de4: ad4c4006     	ldp	q6, q16, [x0, #0x180]
     de8: ad424811     	ldp	q17, q18, [x0, #0x40]
     dec: 4e31d4c6     	fadd.4s	v6, v6, v17
     df0: ad4e4c11     	ldp	q17, q19, [x0, #0x1c0]
     df4: ad445414     	ldp	q20, q21, [x0, #0x80]
     df8: 4e34d631     	fadd.4s	v17, v17, v20
     dfc: ad505814     	ldp	q20, q22, [x0, #0x200]
     e00: ad466017     	ldp	q23, q24, [x0, #0xc0]
     e04: 4e37d694     	fadd.4s	v20, v20, v23
     e08: ad526417     	ldp	q23, q25, [x0, #0x240]
     e0c: ad486c1a     	ldp	q26, q27, [x0, #0x100]
     e10: 4e3ad6f7     	fadd.4s	v23, v23, v26
     e14: 4e27d4a5     	fadd.4s	v5, v5, v7
     e18: ad001404     	stp	q4, q5, [x0]
     e1c: 4e32d604     	fadd.4s	v4, v16, v18
     e20: ad021006     	stp	q6, q4, [x0, #0x40]
     e24: 4e35d664     	fadd.4s	v4, v19, v21
     e28: ad041011     	stp	q17, q4, [x0, #0x80]
     e2c: 4e38d6c4     	fadd.4s	v4, v22, v24
     e30: ad061014     	stp	q20, q4, [x0, #0xc0]
     e34: 4e3bd724     	fadd.4s	v4, v25, v27
     e38: ad081017     	stp	q23, q4, [x0, #0x100]
     e3c: ad4b1404     	ldp	q4, q5, [x0, #0x160]
     e40: ad411c06     	ldp	q6, q7, [x0, #0x20]
     e44: 4e26d484     	fadd.4s	v4, v4, v6
     e48: ad4d4006     	ldp	q6, q16, [x0, #0x1a0]
     e4c: ad434811     	ldp	q17, q18, [x0, #0x60]
     e50: 4e31d4c6     	fadd.4s	v6, v6, v17
     e54: ad4f4c11     	ldp	q17, q19, [x0, #0x1e0]
     e58: ad455414     	ldp	q20, q21, [x0, #0xa0]
     e5c: 4e34d631     	fadd.4s	v17, v17, v20
     e60: ad515814     	ldp	q20, q22, [x0, #0x220]
     e64: ad476017     	ldp	q23, q24, [x0, #0xe0]
     e68: 4e37d694     	fadd.4s	v20, v20, v23
     e6c: ad536417     	ldp	q23, q25, [x0, #0x260]
     e70: ad496c1a     	ldp	q26, q27, [x0, #0x120]
     e74: 4e3ad6f7     	fadd.4s	v23, v23, v26
     e78: 4e27d4a5     	fadd.4s	v5, v5, v7
     e7c: ad011404     	stp	q4, q5, [x0, #0x20]
     e80: 4e32d604     	fadd.4s	v4, v16, v18
     e84: ad031006     	stp	q6, q4, [x0, #0x60]
     e88: 4e35d664     	fadd.4s	v4, v19, v21
     e8c: ad051011     	stp	q17, q4, [x0, #0xa0]
     e90: 4e38d6c4     	fadd.4s	v4, v22, v24
     e94: ad071014     	stp	q20, q4, [x0, #0xe0]
     e98: 4e3bd724     	fadd.4s	v4, v25, v27
     e9c: ad091017     	stp	q23, q4, [x0, #0x120]
     ea0: ad000500     	stp	q0, q1, [x8]
     ea4: ad010d02     	stp	q2, q3, [x8, #0x20]
     ea8: d65f03c0     	ret
