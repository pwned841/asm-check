section .text
wide:
    xor rax, rax            ;~
    xor rdi, rdi            ;~
    xor r8, r8
    xor eax, eax
    xor rax, rbx
    mov rax, 1
    ret
wider:
    movzx rax, byte [rsi]   ;~
    movzx rcx, dl           ;~
    movzx eax, byte [rsi]
    movzx r8, bl
    movzx rax, byte [r9]
    movzx rax, word [rdi + rcx*2] ;~
    ret
