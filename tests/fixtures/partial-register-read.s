global _start
section .text
_start:
    mov al, [rsp]
    cmp rax, 10             ;~
    xor ebx, ebx
    mov bl, [rsp]
    cmp rbx, 10
    mov cl, 1
    cmp cl, 1
    movzx edx, cl
    mov si, 5
    add esi, 1              ;~
    mov dil, 60
    mov eax, 60
    syscall
