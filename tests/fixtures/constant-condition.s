section .text
f:
    mov eax, 5
    cmp eax, 5
    jne .a                  ;~
    mov rbx, 3
    cmp rbx, 10
    jl .a                   ;~
    xor ecx, ecx
    test ecx, ecx
    jz .a                   ;~
    mov rdx, -1
    cmp rdx, 1
    jb .a                   ;~
    jl .a                   ;~
    cmp rsi, 5
    jl .a
    mov r8d, 7
.loop:
    inc r8d
    cmp r8d, 10
    jl .loop
    call g
    cmp rax, 0
    je .a
.a:
    ret
g:
    mov rax, rdi
    ret
