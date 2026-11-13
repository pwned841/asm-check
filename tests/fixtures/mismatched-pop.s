global swapped, ordered, moved
section .text
swapped:
    push rbx
    push r12
    pop rbx                 ;~
    pop r12
    ret
ordered:
    push rbx
    push r12
    pop r12
    pop rbx
    ret
moved:
    push rax
    pop rdi
    push rsi
    push rdx
    pop rax
    pop rcx
    ret
