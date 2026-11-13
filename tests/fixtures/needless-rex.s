section .text
wide:
    mov rax, 1              ;~
    mov r9, 60              ;~
    mov eax, 1
    mov rax, -1
    mov rax, 0x100000000
    mov rax, msg
    xor rax, rax            ;~
    xor r8, r8
    xor eax, eax
    ret

section .data
msg db 1
