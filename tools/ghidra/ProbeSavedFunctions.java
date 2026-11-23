import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.program.model.listing.Function;
import java.io.PrintWriter;
public class ProbeSavedFunctions extends GhidraScript {
 public void run() throws Exception {
  if (!"a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56".equals(currentProgram.getExecutableSHA256())) throw new Exception("binary identity mismatch");
  String[] args=getScriptArgs();
  DecompInterface decompiler=new DecompInterface();
  try(PrintWriter out=new PrintWriter(args[0])) {
   if(!decompiler.openProgram(currentProgram))throw new Exception(decompiler.getLastMessage());
   for(int i=1;i<args.length;i++) {
    Function f=currentProgram.getFunctionManager().getFunctionAt(toAddr(args[i]));
    if(f==null)throw new Exception("function absent at "+args[i]);
    var result=decompiler.decompileFunction(f,60,monitor);
    if(!result.decompileCompleted())throw new Exception(result.getErrorMessage());
    out.println("// RVA evidence: "+args[i]+" "+f.getName());
    out.println(result.getDecompiledFunction().getC());
    println("RECOVERED "+args[i]+" "+f.getName());
   }
  }finally{decompiler.dispose();}
 }
}
