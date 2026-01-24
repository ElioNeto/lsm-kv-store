import json
import random
import uuid
import hashlib
import string
import os
from datetime import datetime

def get_random_string(length):
    return ''.join(random.choices(string.ascii_letters + string.digits + "   .,!?-", k=length))

def generate_complex_data(target_mb=10, file_name="stress_test_data.json"):
    records = []
    current_size = 0
    target_bytes = target_mb * 1024 * 1024
    
    categories = ["identity", "telemetry", "audit_log", "blob_meta", "session_cache"]
    
    print(f"🚀 Gerando {target_mb}MB de dados com alta entropia...")

    while current_size < target_bytes:
        cat = random.choice(categories)
        
        # --- Cenário 1: Identidade (JSON médio) ---
        if cat == "identity":
            uid = str(uuid.uuid4())
            key = f"user:profile:{uid[:8]}"
            val_obj = {
                "uid": uid,
                "token": hashlib.sha256(uid.encode()).hexdigest(),
                "bio": get_random_string(random.randint(200, 1000)),
                "roles": random.sample(["admin", "user", "guest", "manager", "support"], 2),
                "active": random.choice([True, False]),
                "metadata": {"last_ip": f"{random.randint(1,255)}.{random.randint(1,255)}.0.1"}
            }
            value = json.dumps(val_obj)

        # --- Cenário 2: Telemetria (Curto e denso) ---
        elif cat == "telemetry":
            key = f"sensor:th:{random.randint(1000, 9999)}"
            value = f"t={random.uniform(18.0, 42.0):.2f};h={random.uniform(30, 90):.2f};st={datetime.now().isoformat()}"

        # --- Cenário 3: Audit Log (Longas strings/Stack traces) ---
        elif cat == "audit_log":
            key = f"log:{datetime.now().strftime('%Y%m%d')}:{uuid.uuid4().hex[:6]}"
            # Simula um erro do sistema com "stack trace"
            trace = " | ".join([get_random_string(100) for _ in range(random.randint(10, 50))])
            value = f"ERROR: OutOfMemoryException at {datetime.now()} in Module {get_random_string(10)}. Context: {trace}"

        # --- Cenário 4: Session Cache (Tokens JWT simulados) ---
        elif cat == "session_cache":
            key = f"sess:{hashlib.md5(str(random.random()).encode()).hexdigest()}"
            # Simula um header.payload.signature
            header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
            payload = hashlib.sha512(get_random_string(50).encode()).hexdigest()
            sig = hashlib.sha1(get_random_string(20).encode()).hexdigest()
            value = f"{header}.{payload}.{sig}"

        # --- Cenário 5: Blob Metadata (Objeto denso) ---
        else:
            key = f"file:meta:{random.getrandbits(32)}"
            value = json.dumps({
                "filename": f"{get_random_string(10)}.pdf",
                "checksum": hashlib.md5(get_random_string(100).encode()).hexdigest(),
                "tags": [get_random_string(5) for _ in range(10)],
                "flags": [random.randint(0, 1000) for _ in range(20)],
                "description": get_random_string(random.randint(1000, 5000)) # Valor maior para forçar o flush
            })

        record = {"key": key, "value": value}
        records.append(record)
        current_size += len(key) + len(value) + 50 # Estimativa de overhead do JSON

    with open(file_name, "w", encoding="utf-8") as f:
        json.dump({"records": records}, f, ensure_ascii=False)

    final_size = os.path.getsize(file_name) / (1024 * 1024)
    print(f"✅ Arquivo '{file_name}' criado.")
    print(f"📊 Total de registros: {len(records)}")
    print(f"📦 Tamanho final: {final_size:.2f} MB")

if __name__ == "__main__":
    generate_complex_data(10) # 10MB para garantir o estouro da memtable de 4MB