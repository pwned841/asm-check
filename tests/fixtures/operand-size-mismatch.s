section .text
sizes:
    mov rax, ebx            ;~
    mov eax, ebx
    mov al, bx              ;~
    add rax, cl             ;~
    cmp eax, r8d
    cmp rax, dword [rdi]    ;~
    mov rax, qword [rdi]
    mov [rdi], rax
    movzx rax, bl
    movzx eax, ebx          ;~
    movzx rax, ebx          ;~
    movsx rax, byte [rsi]
    shl rax, cl
    shl rax, 3
    shl rax, rcx            ;~
    shr rax, bl             ;~
    cmovg rax, ebx          ;~
    mov rax, cr0
    mov ds, ax
    ret
