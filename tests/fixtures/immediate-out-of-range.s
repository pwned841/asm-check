section .text
f:
    mov al, 300                 ;~
    mov al, 255
    mov al, -128
    mov al, -129                ;~
    cmp bl, 256                 ;~
    mov byte [rdi], 1000        ;~
    mov ax, 70000               ;~
    mov eax, 0x100000000        ;~
    add eax, 0xFFFFFFFF
    and rax, 0xFFFFFFFF         ;~
    cmp rax, 0x7FFFFFFF
    add rax, 0x100000000        ;~
    mov rax, 0x100000000
    mov qword [rdi], 0x100000000 ;~
    and rax, 0xFFFFFFFFFFFFFFFF
    add rax, -1
    ret
