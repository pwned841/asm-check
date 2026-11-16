section .text
operands:
    mov [first], [second]           ;~
    cmp byte [rdi], [rsi]           ;~
    mov counter, rax                ;~
    mov 5, rax                      ;~
    cmp 5, rax                      ;~
    inc 5                           ;~
    lea rax, rbx                    ;~
    lea rax, [rbx]
    push eax                        ;~
    pop bl                          ;~
    push rax
    push ax
    mov rax                         ;~
    syscall rax                     ;~
    ret 8
    bsf al, bl                      ;~
    popcnt rax, 5                   ;~
    xadd rax, [rdi]                 ;~
    bt al, 1                        ;~
    imul rax, rbx, rcx              ;~
    imul 5                          ;~
    movzx rax, 5                    ;~
    setz rax                        ;~
    bswap ax                        ;~
    movsxd eax, ebx                 ;~
    push dword [rdi]                ;~
    call word [rdi]                 ;~
    shl rax, [rdi]                  ;~
    cmovz al, bl                    ;~
    bsf rax, rbx
    popcnt eax, [rdi]
    xadd [rdi], rax
    bt rax, 5
    bt [rdi], rax
    imul rax, rbx, 7
    imul rax, [rdi]
    movzx eax, byte [rdi]
    movsx rax, eax
    movsxd rax, ebx
    setz al
    setz byte [rdi]
    bswap eax
    push qword [rdi]
    call qword [rdi]
    cmovz rax, rbx
    lea rax, 8
    lea rax, first

section .data
first dq 0
second dq 0
counter dq 0
