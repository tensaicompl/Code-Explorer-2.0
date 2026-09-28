package body Greeters is
   function Greet (Name : String) return String is
   begin
      return "hi " & Name;
   end Greet;
end Greeters;
