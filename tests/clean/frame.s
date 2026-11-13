; Library-style functions: a stack frame, a signed division, a string copy.
global average, copy

section .text
; long average(const long *values, long count)
average:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov qword [rbp - 8], 0
    xor ecx, ecx
.sum:
    cmp rcx, rsi
    jge .done
    mov rax, [rdi + rcx*8]
    add [rbp - 8], rax
    inc rcx
    jmp .sum
.done:
    mov rax, [rbp - 8]
    cqo
    idiv rsi
    leave
    ret

; void copy(char *dst, const char *src, size_t n)
copy:
    mov rcx, rdx
    rep movsb
    ret
