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

section .data
first dq 0
second dq 0
counter dq 0
