; Standalone aggregate control, with no Rust typestate or PhantomData.
; Run: opt -S -passes=instcombine investigation/aggregate_copy.ll -o -
target datalayout = "e-m:e-p:64:64-i64:64-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i64 @opaque_drop(ptr noalias readonly captures(none) align 8 dereferenceable(56) %src) {
entry:
  %dst = alloca [56 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src, i64 56, i1 false)
  %slot = getelementptr inbounds i8, ptr %dst, i64 24
  %x = load i64, ptr %slot, align 8
  call void @drop_unknown(ptr noalias %dst)
  ret i64 %x
}

define i64 @known_readonly_drop(ptr noalias readonly captures(none) align 8 dereferenceable(56) %src) {
entry:
  %dst = alloca [56 x i8], align 8
  call void @llvm.memcpy.p0.p0.i64(ptr align 8 %dst, ptr align 8 %src, i64 56, i1 false)
  %slot = getelementptr inbounds i8, ptr %dst, i64 24
  %x = load i64, ptr %slot, align 8
  call void @drop_known(ptr noalias readonly captures(none) %dst)
  ret i64 %x
}

declare void @drop_unknown(ptr noalias)
declare void @drop_known(ptr noalias readonly captures(none))
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
