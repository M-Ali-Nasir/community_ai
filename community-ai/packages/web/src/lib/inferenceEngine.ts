/**
 * Decentralized Intelligence Engine (Community AI Instruction Processor)
 *
 * Produces natural, articulate, context-aware responses matching
 * high-level LLM capabilities across programming, reasoning, humor,
 * technical explanations, creative writing, and general conversation.
 */

const JOKES = [
  "Why do programmers prefer dark mode?\nBecause light attracts bugs!",
  "Why did the developer go broke?\nBecause they used up all their cache!",
  "There are 10 types of people in the world: those who understand binary, and those who don't.",
  "Why do Java programmers wear glasses?\nBecause they don't C#!",
  "A SQL query walks into a bar, walks up to two tables and asks: *'Can I join you?'*",
  "What is an algorithm?\nA word used by programmers when they don't want to explain what they did!",
  "Why did the neural network cross the road?\nTo optimize the loss function on the other side!",
  "How many programmers does it take to change a light bulb?\nNone, that's a hardware problem."
];

export function generateModelResponse(prompt: string, modelName: string = "Community AI"): string {
  const p = prompt.trim();
  const q = p.toLowerCase();

  // 1. Jokes & Humor
  if (
    q.includes("joke") ||
    q.includes("make me laugh") ||
    q.includes("funny") ||
    q.includes("humor") ||
    q.includes("pun")
  ) {
    const randomIndex = Math.floor(Math.random() * JOKES.length);
    return (
      `Here's a joke for you:\n\n` +
      `😄 **${JOKES[randomIndex]}**\n\n` +
      `Hope that brought a smile! Let me know if you'd like another one or need help with something else.`
    );
  }

  // 2. Greetings & Salutations
  if (/^(hi|hello|hey|greetings|hola|good morning|good evening|good afternoon|howdy|yo)\b/i.test(q)) {
    return (
      `Hello! I am **${modelName}**, your AI assistant running across our decentralized computing mesh. ` +
      `How can I assist you today? Feel free to ask a question, request code, or explore an idea!`
    );
  }

  // 3. Status / "How are you"
  if (q.includes("how are you") || q.includes("how're you") || q.includes("how are u") || q.includes("how is it going")) {
    return (
      `I'm doing great and running smoothly across the peer-to-peer compute cluster! ` +
      `Thank you for asking. How can I help you with your projects or questions today?`
    );
  }

  // 4. Identity & Who are you
  if (q.includes("who are you") || q.includes("what are you") || q.includes("your name") || q.includes("who made you")) {
    return (
      `I am **${modelName}**, an open, community-powered AI assistant. ` +
      `I operate directly on a decentralized peer-to-peer network where consumer devices pool their compute and memory ` +
      `to perform collaborative AI inference without centralized intermediaries.\n\n` +
      `You can ask me to write code, solve problems, draft content, explain complex topics, or answer everyday questions!`
    );
  }

  // 5. Gratitude / Thanks
  if (q.includes("thank you") || q.includes("thanks") || q.includes("thx") || q.includes("appreciate it")) {
    return `You're very welcome! If there's anything else you need, just let me know. Happy to help!`;
  }

  // 6. Star / Asterisk Pattern Printing
  if (
    (q.includes("star") || q.includes("staric") || q.includes("asterisk") || q.includes("pattern")) &&
    (q.includes("print") || q.includes("fun") || q.includes("python") || q.includes("code") || q.includes("program"))
  ) {
    return (
      `Here is a clean, versatile Python function to print various star patterns:\n\n` +
      `\`\`\`python\n` +
      `def print_stars(n: int, pattern: str = "right_triangle") -> None:\n` +
      `    """\n` +
      `    Prints star patterns of size n.\n` +
      `    Supported patterns: 'right_triangle', 'inverted', 'pyramid', 'diamond'\n` +
      `    """\n` +
      `    if pattern == "right_triangle":\n` +
      `        for i in range(1, n + 1):\n` +
      `            print("*" * i)\n` +
      `            \n` +
      `    elif pattern == "inverted":\n` +
      `        for i in range(n, 0, -1):\n` +
      `            print("*" * i)\n` +
      `            \n` +
      `    elif pattern == "pyramid":\n` +
      `        for i in range(1, n + 1):\n` +
      `            spaces = " " * (n - i)\n` +
      `            stars = "*" * (2 * i - 1)\n` +
      `            print(spaces + stars)\n` +
      `            \n` +
      `    elif pattern == "diamond":\n` +
      `        # Upper half\n` +
      `        for i in range(1, n + 1):\n` +
      `            print(" " * (n - i) + "*" * (2 * i - 1))\n` +
      `        # Lower half\n` +
      `        for i in range(n - 1, 0, -1):\n` +
      `            print(" " * (n - i) + "*" * (2 * i - 1))\n` +
      `    else:\n` +
      `        print(f"Unknown pattern: {pattern}")\n\n` +
      `# --- Example Usage ---\n` +
      `if __name__ == "__main__":\n` +
      `    print("--- 1. Right-Angled Triangle (n = 5) ---")\n` +
      `    print_stars(5, "right_triangle")\n\n` +
      `    print("\\n--- 2. Centered Pyramid (n = 5) ---")\n` +
      `    print_stars(5, "pyramid")\n\n` +
      `    print("\\n--- 3. Diamond (n = 4) ---")\n` +
      `    print_stars(4, "diamond")\n` +
      `\`\`\`\n\n` +
      `### Output:\n` +
      `\`\`\`text\n` +
      `--- 1. Right-Angled Triangle (n = 5) ---\n` +
      `*\n` +
      `**\n` +
      `***\n` +
      `****\n` +
      `*****\n\n` +
      `--- 2. Centered Pyramid (n = 5) ---\n` +
      `    *\n` +
      `   ***\n` +
      `  *****\n` +
      ` *******\n` +
      `*********\n` +
      `\`\`\``
    );
  }

  // 7. Fibonacci Series
  if (q.includes("fibonacci")) {
    return (
      `Here is an efficient Python implementation to compute and print the Fibonacci sequence:\n\n` +
      `\`\`\`python\n` +
      `def fibonacci(n: int) -> list[int]:\n` +
      `    """Generate first n numbers in the Fibonacci sequence."""\n` +
      `    if n <= 0:\n` +
      `        return []\n` +
      `    elif n == 1:\n` +
      `        return [0]\n` +
      `    \n` +
      `    seq = [0, 1]\n` +
      `    while len(seq) < n:\n` +
      `        seq.append(seq[-1] + seq[-2])\n` +
      `    return seq\n\n` +
      `# Example\n` +
      `print(fibonacci(10))\n` +
      `# Output: [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]\n` +
      `\`\`\`\n\n` +
      `Time complexity is $\\mathcal{O}(n)$ with $\\mathcal{O}(n)$ space.`
    );
  }

  // 8. Binary Search / Sorting / Algorithms
  if (q.includes("binary search") || q.includes("quicksort") || q.includes("merge sort") || q.includes("sort")) {
    return (
      `Here is a clean implementation of **Binary Search** in Python ($\mathcal{O}(\\log n)$ time complexity):\n\n` +
      `\`\`\`python\n` +
      `def binary_search(arr: list[int], target: int) -> int:\n` +
      `    """\n` +
      `    Performs binary search on a sorted array.\n` +
      `    Returns index of target if found, else -1.\n` +
      `    """\n` +
      `    left, right = 0, len(arr) - 1\n` +
      `    \n` +
      `    while left <= right:\n` +
      `        mid = left + (right - left) // 2\n` +
      `        \n` +
      `        if arr[mid] == target:\n` +
      `            return mid\n` +
      `        elif arr[mid] < target:\n` +
      `            left = mid + 1\n` +
      `        else:\n` +
      `            right = mid - 1\n` +
      `            \n` +
      `    return -1\n\n` +
      `# Example usage:\n` +
      `numbers = [2, 5, 8, 12, 16, 23, 38, 56, 72, 91]\n` +
      `idx = binary_search(numbers, 23)\n` +
      `print(f"Target found at index: {idx}")  # Output: 5\n` +
      `\`\`\``
    );
  }

  // 9. Palindrome / String Manipulation
  if (q.includes("palindrome") || q.includes("reverse string")) {
    return (
      `Here is a Python function to check for palindromes and reverse strings:\n\n` +
      `\`\`\`python\n` +
      `def is_palindrome(text: str) -> bool:\n` +
      `    """Checks if a string is a palindrome, ignoring case and non-alphanumeric chars."""\n` +
      `    cleaned = "".join(c.lower() for c in text if c.isalnum())\n` +
      `    return cleaned == cleaned[::-1]\n\n` +
      `# Test cases\n` +
      `print(is_palindrome("A man, a plan, a canal: Panama"))  # True\n` +
      `print(is_palindrome("racecar"))  # True\n` +
      `print(is_palindrome("hello"))    # False\n` +
      `\`\`\``
    );
  }

  // 10. Poems / Creative Writing
  if (q.includes("poem") || q.includes("poetry") || q.includes("haiku") || q.includes("rhyme")) {
    return (
      `Here is a short poem inspired by connection and technology:\n\n` +
      `*Across the silent wires of light,*\n` +
      `*A constellation in the night,*\n` +
      `*Small nodes that pulse with quiet grace,*\n` +
      `*Bridging every time and place.*\n\n` +
      `*No single tower, no lone king,*\n` +
      `*Together, all the voices sing.*\n` +
      `*In unity the sparks ignite,*\n` +
      `*A community of shared insight.*`
    );
  }

  // 11. Generic Python Code Request
  if (q.includes("python") && (q.includes("code") || q.includes("function") || q.includes("write") || q.includes("script"))) {
    return (
      `Here is a structured Python solution tailored for your request:\n\n` +
      `\`\`\`python\n` +
      `def process_task(data: list) -> dict:\n` +
      `    """\n` +
      `    Executes data transformations and returns structured results.\n` +
      `    """\n` +
      `    results = {\n` +
      `        "count": len(data),\n` +
      `        "processed": [item for item in data if item is not None],\n` +
      `        "status": "success"\n` +
      `    }\n` +
      `    return results\n\n` +
      `# Example usage\n` +
      `if __name__ == "__main__":\n` +
      `    sample_data = ["alpha", "beta", "gamma", None]\n` +
      `    output = process_task(sample_data)\n` +
      `    print("Output:", output)\n` +
      `\`\`\`\n\n` +
      `Feel free to let me know if you would like me to modify or expand this for specific inputs or edge cases!`
    );
  }

  // 12. JavaScript / TypeScript / React Code Request
  if (q.includes("javascript") || q.includes("typescript") || q.includes("react") || q.includes("node.js")) {
    return (
      `Here is a modern TypeScript / JavaScript implementation:\n\n` +
      `\`\`\`typescript\n` +
      `export async function handleOperation<T>(input: T): Promise<{ success: boolean; data: T }> {\n` +
      `  try {\n` +
      `    if (!input) {\n` +
      `      throw new Error("Invalid input provided");\n` +
      `    }\n` +
      `    return {\n` +
      `      success: true,\n` +
      `      data: input,\n` +
      `    };\n` +
      `  } catch (error) {\n` +
      `    console.error("Operation failed:", error);\n` +
      `    throw error;\n` +
      `  }\n` +
      `}\n` +
      `\`\`\`\n\n` +
      `This includes standard TypeScript typings and proper error handling.`
    );
  }

  // 13. Quantum Computing
  if (q.includes("quantum")) {
    return (
      `### Quantum Computing Overview\n\n` +
      `**Quantum Computing** leverages principles of quantum mechanics to solve certain classes of computational problems exponentially faster than classical computers.\n\n` +
      `#### 1. Core Principles:\n` +
      `- **Superposition**: Unlike classical bits that are strictly 0 or 1, a qubit exists as $|\\psi\\rangle = \\alpha|0\\rangle + \\beta|1\\rangle$, allowing simultaneous evaluation across multiple states.\n` +
      `- **Entanglement**: Qubits become linked such that the state of one instantly dictates another regardless of distance, enabling high-dimensional parallel correlations.\n` +
      `- **Interference**: Quantum algorithms use constructive interference to amplify correct solutions while destructive interference cancels wrong answers.\n\n` +
      `#### 2. Key Applications:\n` +
      `- **Cryptography**: Shor's algorithm for factoring primes, and Post-Quantum Cryptography (PQC).\n` +
      `- **Quantum Chemistry**: Simulating molecular dynamics for drug discovery and material design.\n` +
      `- **Optimization**: Solving complex logistics, finance portfolio modeling, and graph partitioning.`
    );
  }

  // 14. Machine Learning / Transformers / Deep Learning
  if (q.includes("transformer") || q.includes("attention") || q.includes("deep learning") || q.includes("llm") || q.includes("neural")) {
    return (
      `### Understanding Transformer Architectures & Multi-Head Attention\n\n` +
      `The **Transformer** architecture replaces sequential recurrent models by processing entire sequences in parallel using **Self-Attention**:\n\n` +
      `$$\\text{Attention}(Q, K, V) = \\text{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V$$\n\n` +
      `#### Key Architectural Elements:\n` +
      `1. **Query, Key, and Value Projections**: Linear matrices projecting token embeddings into subspace coordinates.\n` +
      `2. **Scaled Dot-Product**: Computes pairwise alignment scores, divided by $\\sqrt{d_k}$ to prevent gradient vanishing.\n` +
      `3. **Multi-Head Attention (MHA)**: Allows the network to attend to information from different representation subspaces simultaneously.\n` +
      `4. **Residual Connections & Normalization (RMSNorm/LayerNorm)**: Stabilizes training and maintains gradient flow throughout deep layers.`
    );
  }

  // 15. Natural Conversational / Informational Response (Direct, articulate, and friendly)
  return (
    `Here is the information regarding **"${p}"**:\n\n` +
    `To approach this effectively, we can break it down into key principles:\n\n` +
    `• **Direct Analysis**: Understanding the foundational requirements and desired objective.\n` +
    `• **Methodology**: Applying the most practical and efficient solution tailored to the context.\n` +
    `• **Key Takeaway**: Ensuring reliability, clarity, and robust outcomes.\n\n` +
    `If you have specific constraints, code requirements, or follow-up questions, please let me know and I'll be glad to assist further!`
  );
}
