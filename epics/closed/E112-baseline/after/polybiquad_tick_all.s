000000000000026c <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17h56aa26fb75526cf6E>:
     26c: ad400400     	ldp	q0, q1, [x0]
     270: ad401427     	ldp	q7, q5, [x1]
     274: 6e27dc00     	fmul.4s	v0, v0, v7
     278: ad540c02     	ldp	q2, q3, [x0, #0x280]
     27c: 4e22d400     	fadd.4s	v0, v0, v2
     280: 6e25dc21     	fmul.4s	v1, v1, v5
     284: 4e23d421     	fadd.4s	v1, v1, v3
     288: ad410c02     	ldp	q2, q3, [x0, #0x20]
     28c: ad411026     	ldp	q6, q4, [x1, #0x20]
     290: 6e26dc42     	fmul.4s	v2, v2, v6
     294: ad554410     	ldp	q16, q17, [x0, #0x2a0]
     298: 4e30d442     	fadd.4s	v2, v2, v16
     29c: 6e24dc63     	fmul.4s	v3, v3, v4
     2a0: 4e31d463     	fadd.4s	v3, v3, v17
     2a4: 4ea01c10     	mov.16b	v16, v0
     2a8: 4ea11c31     	mov.16b	v17, v1
     2ac: 4ea21c52     	mov.16b	v18, v2
     2b0: 4ea31c73     	mov.16b	v19, v3
     2b4: 34000cc2     	cbz	w2, 0x44c <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17h56aa26fb75526cf6E+0x1e0>
     2b8: 6dbe2beb     	stp	d11, d10, [sp, #-0x20]!
     2bc: 6d0123e9     	stp	d9, d8, [sp, #0x10]
     2c0: 4f00f493     	fmov.4s	v19, #2.50000000
     2c4: 6e33e410     	fcmge.4s	v16, v0, v19
     2c8: 4f04f495     	fmov.4s	v21, #-2.50000000
     2cc: 6e20e6b1     	fcmge.4s	v17, v21, v0
     2d0: 6e20dc12     	fmul.4s	v18, v0, v0
     2d4: 6e32de5a     	fmul.4s	v26, v18, v18
     2d8: 6e3ade5b     	fmul.4s	v27, v18, v26
     2dc: 52900009     	mov	w9, #0x8000             ; =32768
     2e0: 72a893a9     	movk	w9, #0x449d, lsl #16
     2e4: 4e040d36     	dup.4s	v22, w9
     2e8: 6e36de57     	fmul.4s	v23, v18, v22
     2ec: 528d8009     	mov	w9, #0x6c00             ; =27648
     2f0: 72a8c449     	movk	w9, #0x4622, lsl #16
     2f4: 4e040d34     	dup.4s	v20, w9
     2f8: 4e34d6f8     	fadd.4s	v24, v23, v20
     2fc: 4f01f6b7     	fmov.4s	v23, #21.00000000
     300: 6e37df59     	fmul.4s	v25, v26, v23
     304: 4e39d718     	fadd.4s	v24, v24, v25
     308: 6e38dc1c     	fmul.4s	v28, v0, v24
     30c: 52950009     	mov	w9, #0xa800             ; =43008
     310: 72a8b269     	movk	w9, #0x4593, lsl #16
     314: 4e040d38     	dup.4s	v24, w9
     318: 6e38de52     	fmul.4s	v18, v18, v24
     31c: 52a86a49     	mov	w9, #0x43520000         ; =1129447424
     320: 4e040d39     	dup.4s	v25, w9
     324: 4e34d652     	fadd.4s	v18, v18, v20
     328: 6e39df5a     	fmul.4s	v26, v26, v25
     32c: 4e3ad652     	fadd.4s	v18, v18, v26
     330: 4f00f61a     	fmov.4s	v26, #4.00000000
     334: 6e3adf7b     	fmul.4s	v27, v27, v26
     338: 4e3bd652     	fadd.4s	v18, v18, v27
     33c: 6e32ff92     	fdiv.4s	v18, v28, v18
     340: 4e701e31     	bic.16b	v17, v17, v16
     344: 4f07f61b     	fmov.4s	v27, #-1.00000000
     348: 6e721f71     	bsl.16b	v17, v27, v18
     34c: 4f03f61c     	fmov.4s	v28, #1.00000000
     350: 6e711f90     	bsl.16b	v16, v28, v17
     354: 6e33e431     	fcmge.4s	v17, v1, v19
     358: 6e21e6b2     	fcmge.4s	v18, v21, v1
     35c: 6e21dc3d     	fmul.4s	v29, v1, v1
     360: 6e3ddfbe     	fmul.4s	v30, v29, v29
     364: 6e3edfbf     	fmul.4s	v31, v29, v30
     368: 6e36dfa8     	fmul.4s	v8, v29, v22
     36c: 4e34d508     	fadd.4s	v8, v8, v20
     370: 6e37dfc9     	fmul.4s	v9, v30, v23
     374: 4e29d508     	fadd.4s	v8, v8, v9
     378: 6e28dc28     	fmul.4s	v8, v1, v8
     37c: 6e38dfbd     	fmul.4s	v29, v29, v24
     380: 4e34d7bd     	fadd.4s	v29, v29, v20
     384: 6e39dfde     	fmul.4s	v30, v30, v25
     388: 4e3ed7bd     	fadd.4s	v29, v29, v30
     38c: 6e3adffe     	fmul.4s	v30, v31, v26
     390: 4e3ed7bd     	fadd.4s	v29, v29, v30
     394: 6e3dfd1d     	fdiv.4s	v29, v8, v29
     398: 4e711e52     	bic.16b	v18, v18, v17
     39c: 6e7d1f72     	bsl.16b	v18, v27, v29
     3a0: 6e721f91     	bsl.16b	v17, v28, v18
     3a4: 6e33e452     	fcmge.4s	v18, v2, v19
     3a8: 6e22e6bd     	fcmge.4s	v29, v21, v2
     3ac: 6e22dc5e     	fmul.4s	v30, v2, v2
     3b0: 6e3edfdf     	fmul.4s	v31, v30, v30
     3b4: 6e3fdfc8     	fmul.4s	v8, v30, v31
     3b8: 6e36dfc9     	fmul.4s	v9, v30, v22
     3bc: 4e34d529     	fadd.4s	v9, v9, v20
     3c0: 6e37dfea     	fmul.4s	v10, v31, v23
     3c4: 4e2ad529     	fadd.4s	v9, v9, v10
     3c8: 6e29dc49     	fmul.4s	v9, v2, v9
     3cc: 6e38dfde     	fmul.4s	v30, v30, v24
     3d0: 4e34d7de     	fadd.4s	v30, v30, v20
     3d4: 6e39dfff     	fmul.4s	v31, v31, v25
     3d8: 4e3fd7de     	fadd.4s	v30, v30, v31
     3dc: 6e3add1f     	fmul.4s	v31, v8, v26
     3e0: 4e3fd7de     	fadd.4s	v30, v30, v31
     3e4: 6e3efd3e     	fdiv.4s	v30, v9, v30
     3e8: 4e721fbd     	bic.16b	v29, v29, v18
     3ec: 6e7e1f7d     	bsl.16b	v29, v27, v30
     3f0: 6e7d1f92     	bsl.16b	v18, v28, v29
     3f4: 6e33e473     	fcmge.4s	v19, v3, v19
     3f8: 6e23e6b5     	fcmge.4s	v21, v21, v3
     3fc: 6e23dc7d     	fmul.4s	v29, v3, v3
     400: 6e3ddfbe     	fmul.4s	v30, v29, v29
     404: 6e3edfbf     	fmul.4s	v31, v29, v30
     408: 6e36dfb6     	fmul.4s	v22, v29, v22
     40c: 4e34d6d6     	fadd.4s	v22, v22, v20
     410: 6e37dfd7     	fmul.4s	v23, v30, v23
     414: 4e37d6d6     	fadd.4s	v22, v22, v23
     418: 6e36dc76     	fmul.4s	v22, v3, v22
     41c: 6e38dfb7     	fmul.4s	v23, v29, v24
     420: 4e34d6f4     	fadd.4s	v20, v23, v20
     424: 6e39dfd7     	fmul.4s	v23, v30, v25
     428: 4e37d694     	fadd.4s	v20, v20, v23
     42c: 6e3adff7     	fmul.4s	v23, v31, v26
     430: 4e37d694     	fadd.4s	v20, v20, v23
     434: 6e34fed4     	fdiv.4s	v20, v22, v20
     438: 4e731eb5     	bic.16b	v21, v21, v19
     43c: 6eb51f74     	bit.16b	v20, v27, v21
     440: 6e741f93     	bsl.16b	v19, v28, v20
     444: 6d4123e9     	ldp	d9, d8, [sp, #0x10]
     448: 6cc22beb     	ldp	d11, d10, [sp], #0x20
     44c: ad425414     	ldp	q20, q21, [x0, #0x40]
     450: 6e34dcf4     	fmul.4s	v20, v7, v20
     454: ad465c16     	ldp	q22, q23, [x0, #0xc0]
     458: 6e36de16     	fmul.4s	v22, v16, v22
     45c: 4eb6d694     	fsub.4s	v20, v20, v22
     460: ad566016     	ldp	q22, q24, [x0, #0x2c0]
     464: 4e34d6d4     	fadd.4s	v20, v22, v20
     468: 6e35dcb5     	fmul.4s	v21, v5, v21
     46c: 6e37de36     	fmul.4s	v22, v17, v23
     470: 4eb6d6b5     	fsub.4s	v21, v21, v22
     474: 4e35d715     	fadd.4s	v21, v24, v21
     478: ad145414     	stp	q20, q21, [x0, #0x280]
     47c: ad435414     	ldp	q20, q21, [x0, #0x60]
     480: 6e34dcd4     	fmul.4s	v20, v6, v20
     484: ad475c16     	ldp	q22, q23, [x0, #0xe0]
     488: 6e36de56     	fmul.4s	v22, v18, v22
     48c: 4eb6d694     	fsub.4s	v20, v20, v22
     490: ad576016     	ldp	q22, q24, [x0, #0x2e0]
     494: 4e34d6d4     	fadd.4s	v20, v22, v20
     498: 6e35dc95     	fmul.4s	v21, v4, v21
     49c: 6e37de76     	fmul.4s	v22, v19, v23
     4a0: 4eb6d6b5     	fsub.4s	v21, v21, v22
     4a4: 4e35d715     	fadd.4s	v21, v24, v21
     4a8: ad155414     	stp	q20, q21, [x0, #0x2a0]
     4ac: ad445414     	ldp	q20, q21, [x0, #0x80]
     4b0: 6e34dce7     	fmul.4s	v7, v7, v20
     4b4: ad485814     	ldp	q20, q22, [x0, #0x100]
     4b8: 6e34de10     	fmul.4s	v16, v16, v20
     4bc: 4eb0d4e7     	fsub.4s	v7, v7, v16
     4c0: 6e35dca5     	fmul.4s	v5, v5, v21
     4c4: 6e36de30     	fmul.4s	v16, v17, v22
     4c8: 4eb0d4a5     	fsub.4s	v5, v5, v16
     4cc: ad161407     	stp	q7, q5, [x0, #0x2c0]
     4d0: ad451c05     	ldp	q5, q7, [x0, #0xa0]
     4d4: 6e25dcc5     	fmul.4s	v5, v6, v5
     4d8: ad494006     	ldp	q6, q16, [x0, #0x120]
     4dc: 6e26de46     	fmul.4s	v6, v18, v6
     4e0: 4ea6d4a5     	fsub.4s	v5, v5, v6
     4e4: 6e27dc84     	fmul.4s	v4, v4, v7
     4e8: 6e30de66     	fmul.4s	v6, v19, v16
     4ec: 4ea6d484     	fsub.4s	v4, v4, v6
     4f0: ad171005     	stp	q5, q4, [x0, #0x2e0]
     4f4: 36000663     	tbz	w3, #0x0, 0x5c0 <__ZN11patches_dsp6biquad10PolyBiquad8tick_all17h56aa26fb75526cf6E+0x354>
     4f8: ad4a1404     	ldp	q4, q5, [x0, #0x140]
     4fc: ad401c06     	ldp	q6, q7, [x0]
     500: 4e26d484     	fadd.4s	v4, v4, v6
     504: 4e27d4a5     	fadd.4s	v5, v5, v7
     508: ad001404     	stp	q4, q5, [x0]
     50c: ad4b1404     	ldp	q4, q5, [x0, #0x160]
     510: ad411c06     	ldp	q6, q7, [x0, #0x20]
     514: 4e26d484     	fadd.4s	v4, v4, v6
     518: 4e27d4a5     	fadd.4s	v5, v5, v7
     51c: ad011404     	stp	q4, q5, [x0, #0x20]
     520: ad4c1404     	ldp	q4, q5, [x0, #0x180]
     524: ad421c06     	ldp	q6, q7, [x0, #0x40]
     528: 4e26d484     	fadd.4s	v4, v4, v6
     52c: 4e27d4a5     	fadd.4s	v5, v5, v7
     530: ad021404     	stp	q4, q5, [x0, #0x40]
     534: ad4d1404     	ldp	q4, q5, [x0, #0x1a0]
     538: ad431c06     	ldp	q6, q7, [x0, #0x60]
     53c: 4e26d484     	fadd.4s	v4, v4, v6
     540: 4e27d4a5     	fadd.4s	v5, v5, v7
     544: ad031404     	stp	q4, q5, [x0, #0x60]
     548: ad4e1404     	ldp	q4, q5, [x0, #0x1c0]
     54c: ad441c06     	ldp	q6, q7, [x0, #0x80]
     550: 4e26d484     	fadd.4s	v4, v4, v6
     554: 4e27d4a5     	fadd.4s	v5, v5, v7
     558: ad041404     	stp	q4, q5, [x0, #0x80]
     55c: ad4f1404     	ldp	q4, q5, [x0, #0x1e0]
     560: ad451c06     	ldp	q6, q7, [x0, #0xa0]
     564: 4e26d484     	fadd.4s	v4, v4, v6
     568: 4e27d4a5     	fadd.4s	v5, v5, v7
     56c: ad051404     	stp	q4, q5, [x0, #0xa0]
     570: ad501404     	ldp	q4, q5, [x0, #0x200]
     574: ad461c06     	ldp	q6, q7, [x0, #0xc0]
     578: 4e26d484     	fadd.4s	v4, v4, v6
     57c: 4e27d4a5     	fadd.4s	v5, v5, v7
     580: ad061404     	stp	q4, q5, [x0, #0xc0]
     584: ad511404     	ldp	q4, q5, [x0, #0x220]
     588: ad471c06     	ldp	q6, q7, [x0, #0xe0]
     58c: 4e26d484     	fadd.4s	v4, v4, v6
     590: 4e27d4a5     	fadd.4s	v5, v5, v7
     594: ad071404     	stp	q4, q5, [x0, #0xe0]
     598: ad521404     	ldp	q4, q5, [x0, #0x240]
     59c: ad481c06     	ldp	q6, q7, [x0, #0x100]
     5a0: 4e26d484     	fadd.4s	v4, v4, v6
     5a4: 4e27d4a5     	fadd.4s	v5, v5, v7
     5a8: ad081404     	stp	q4, q5, [x0, #0x100]
     5ac: ad531404     	ldp	q4, q5, [x0, #0x260]
     5b0: ad491c06     	ldp	q6, q7, [x0, #0x120]
     5b4: 4e26d484     	fadd.4s	v4, v4, v6
     5b8: 4e27d4a5     	fadd.4s	v5, v5, v7
     5bc: ad091404     	stp	q4, q5, [x0, #0x120]
     5c0: ad000500     	stp	q0, q1, [x8]
     5c4: ad010d02     	stp	q2, q3, [x8, #0x20]
     5c8: d65f03c0     	ret

