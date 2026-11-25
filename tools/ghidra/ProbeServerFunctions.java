import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import java.io.PrintWriter;

/** args: expectedSha256 outFile va... ; defines functions at VAs in an unanalyzed import and decompiles them. */
public class ProbeServerFunctions extends GhidraScript {
 public void run() throws Exception {
  String[] args = getScriptArgs();
  if (!args[0].equals(currentProgram.getExecutableSHA256())) throw new Exception("binary identity mismatch");
  DecompInterface decompiler = new DecompInterface();
  try (PrintWriter out = new PrintWriter(args[1])) {
   if (!decompiler.openProgram(currentProgram)) throw new Exception(decompiler.getLastMessage());
   for (int i = 2; i < args.length; i++) {
    Address a = toAddr(args[i]);
    Function f = currentProgram.getFunctionManager().getFunctionAt(a);
    if (f == null) {
     new DisassembleCommand(a, null, true).applyTo(currentProgram, monitor);
     f = createFunction(a, null);
    }
    if (f == null) throw new Exception("cannot define function at " + args[i]);
    var result = decompiler.decompileFunction(f, 120, monitor);
    if (!result.decompileCompleted()) throw new Exception(result.getErrorMessage());
    out.println("// VA evidence: " + args[i]);
    out.println(result.getDecompiledFunction().getC());
    println("RECOVERED " + args[i]);
   }
  } finally { decompiler.dispose(); }
 }
}
