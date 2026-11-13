global _start
extern strlen
section .text
_start:
    mov rax, [rsp]
    jz .zero                ;~
    cmp rax, 2
    je .zero
    call strlen
    jne .zero               ;~
    test rax, rax
    call helper
    js .zero                ;~
    dec rax
    jnz .zero
    mov eax, 1
    syscall
    jc .zero
.zero:
    cmovz rax, rbx
    mov eax, 60
    syscall
helper:
    ret
